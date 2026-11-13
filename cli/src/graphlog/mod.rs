// Copyright 2020 The Jujutsu Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fmt::Debug;
use std::hash::Hash;
use std::io;

use jj_lib::config::ConfigGetError;
use jj_lib::config::ConfigGetResultExt as _;
use jj_lib::graph::GraphEdge;
use jj_lib::graph::GraphEdgeType;
use jj_lib::settings::UserSettings;
use renderdag::Ancestor;
use renderdag::pipeline::Ascii;
use renderdag::pipeline::AsciiLarge;
use renderdag::pipeline::BoxDrawing;
use renderdag::pipeline::CrossingPriority;
use renderdag::pipeline::GraphRowShaper;
use renderdag::pipeline::PrefixLineRenderer;
use renderdag::pipeline::PrefixLinesToText;
use renderdag::pipeline::coloring::build_line_id_allocator_from_rules;
use renderdag::pipeline::row_shape_to_prefix_lines::box_drawing::Curved;
use renderdag::pipeline::row_shape_to_prefix_lines::box_drawing::Square;

use crate::formatter::Formatter;
use crate::formatter::FormatterExt as _;
use crate::formatter::PlainTextFormatter;
use crate::ui::Ui;

mod branch_drawing;
mod text_writer;

use self::branch_drawing::BranchDrawing;

/// Color labels for graph lines, under the `graph` label. A line has the label
/// that matches its line ID. Twelve labels allow six basic terminal colors and
/// their bold variants.
const LINE_LABELS: [&str; 12] = [
    "line0", "line1", "line2", "line3", "line4", "line5", "line6", "line7", "line8", "line9",
    "line10", "line11",
];

pub trait GraphLog<K: Clone + Eq + Hash> {
    fn add_node(
        &mut self,
        id: &K,
        edges: &[GraphEdge<K>],
        render_node_symbol: &dyn Fn(&mut dyn Formatter) -> io::Result<()>,
        text: &str,
    ) -> io::Result<()>;

    fn width(&self, id: &K, edges: &[GraphEdge<K>]) -> usize;
}

pub struct SaplingGraphLog<'writer, K> {
    shaper: GraphRowShaper<K>,
    prefix_lines: Box<dyn PrefixLineRenderer<K>>,
    text: PrefixLinesToText,
    /// Converts a width in columns to a width in characters.
    columns_to_chars: fn(u64) -> u64,
    formatter: &'writer mut dyn Formatter,
    ui: &'writer Ui,
    /// Whether to draw `○` and `●` nodes as branch drawing commit dots.
    branch_nodes: bool,
}

fn convert_graph_edge_into_ancestor<K: Clone>(e: &GraphEdge<K>) -> Ancestor<K> {
    match e.edge_type {
        GraphEdgeType::Direct => Ancestor::Parent(e.target.clone()),
        GraphEdgeType::Indirect => Ancestor::Ancestor(e.target.clone()),
        GraphEdgeType::Missing => Ancestor::Anonymous,
    }
}

impl<K> GraphLog<K> for SaplingGraphLog<'_, K>
where
    K: Clone + Debug + Eq + Hash,
{
    fn add_node(
        &mut self,
        id: &K,
        edges: &[GraphEdge<K>],
        render_node_symbol: &dyn Fn(&mut dyn Formatter) -> io::Result<()>,
        text: &str,
    ) -> io::Result<()> {
        let row_shape = self.shaper.next_row_shape(
            id.clone(),
            edges.iter().map(convert_graph_edge_into_ancestor).collect(),
        );
        let prefix_lines = self.prefix_lines.next_prefix_lines(&row_shape);
        // Push the graph and line labels before rendering the node so that
        // custom node labels take precedence.
        let node_line_id = row_shape.line_ids.as_ref().map(|ids| {
            ids.before_link[row_shape.node_column]
                .iter()
                .next()
                .unwrap_or_else(|| {
                    // Isolated nodes have no line ID. Use a line label that
                    // doesn't pass through this row.
                    (0..LINE_LABELS.len())
                        .find(|line| {
                            !ids.before_link
                                .iter()
                                .any(|ids| ids.iter().any(|id| id % LINE_LABELS.len() == *line))
                        })
                        .unwrap_or(row_shape.node_column % LINE_LABELS.len())
                })
        });
        let mut glyph = vec![];
        let mut glyph_formatter = self.ui.new_formatter(&mut glyph);
        glyph_formatter.push_label("graph");
        if let Some(id) = node_line_id {
            glyph_formatter.push_label(LINE_LABELS[id % LINE_LABELS.len()]);
        }
        render_node_symbol(glyph_formatter.as_mut())?;
        drop(glyph_formatter);
        let mut glyph = String::from_utf8_lossy(&glyph);
        if self.branch_nodes {
            let mut plain = vec![];
            render_node_symbol(&mut PlainTextFormatter::new(&mut plain))?;
            let plain = String::from_utf8_lossy(&plain);
            let up = !row_shape.is_head;
            let down = !row_shape.is_root;
            if let Some(dot) = branch_node_dot(&plain, up, down) {
                glyph = glyph.replacen(&*plain, dot, 1).into();
            }
        }
        let mut result = Ok(());
        self.text.write_next_lines(
            |text, color| {
                if result.is_ok() {
                    result = write_piece(self.formatter, text, *color);
                }
            },
            prefix_lines,
            row_shape.separator_line,
            &glyph,
            text,
            self.shaper.options().min_row_height,
        );
        result
    }

    fn width(&self, id: &K, edges: &[GraphEdge<K>]) -> usize {
        let parents = edges.iter().map(convert_graph_edge_into_ancestor).collect();
        let columns = self.shaper.width(Some(id), Some(&parents));
        (self.columns_to_chars)(columns).try_into().unwrap()
    }
}

/// The branch drawing commit dot for the node symbol `○` (hollow) or `●`
/// (full), with stubs that join the lines above and below the node.
fn branch_node_dot(symbol: &str, up: bool, down: bool) -> Option<&'static str> {
    let full = match symbol {
        "●" => true,
        "○" => false,
        _ => return None,
    };
    Some(match (full, up, down) {
        (true, false, false) => "\u{f5ee}",
        (false, false, false) => "\u{f5ef}",
        (true, false, true) => "\u{f5f6}",
        (false, false, true) => "\u{f5f7}",
        (true, true, false) => "\u{f5f8}",
        (false, true, false) => "\u{f5f9}",
        (true, true, true) => "\u{f5fa}",
        (false, true, true) => "\u{f5fb}",
    })
}

/// Writes one piece of graph text, with the color label of its line.
fn write_piece(
    formatter: &mut dyn Formatter,
    text: &str,
    line_id: Option<usize>,
) -> io::Result<()> {
    match line_id {
        Some(id) => {
            // Leave the space after a glyph unlabeled, so that a line
            // background color covers only the glyph.
            let glyph = text.trim_end();
            let space = &text[glyph.len()..];
            let line = LINE_LABELS[id % LINE_LABELS.len()];
            write!(formatter.labeled("graph").labeled(line), "{glyph}")?;
            formatter.raw()?.write_all(space.as_bytes())
        }
        // The node symbol, the message text, and the line breaks are already
        // formatted.
        None => formatter.raw()?.write_all(text.as_bytes()),
    }
}

/// The symbols that draw the graph, from `ui.graph.style`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]
#[serde(rename_all(deserialize = "kebab-case"))]
pub enum GraphSymbols {
    Ascii,
    AsciiLarge,
    Curved,
    Square,
    Branch,
}

/// Which line the graph draws where two lines cross.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]
#[serde(rename_all(deserialize = "kebab-case"))]
pub enum GraphCrossing {
    /// The horizontal line hides the vertical line.
    PreferHorizontal,
    /// The vertical line goes through a gap in the horizontal line.
    PreferVertical,
}

/// How a graph is drawn, from the `ui.graph` settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphStyle {
    pub symbols: GraphSymbols,
    /// If `None`, the renderer chooses: the vertical line for `ascii-large`, and
    /// the horizontal line for the other symbol sets.
    pub crossing: Option<GraphCrossing>,
}

impl GraphStyle {
    pub fn from_settings(settings: &UserSettings) -> Result<Self, ConfigGetError> {
        Ok(Self {
            symbols: settings.get("ui.graph.style")?,
            crossing: settings.get("ui.graph.crossing").optional()?,
        })
    }
}

pub fn get_graphlog<'a, K: Clone + Debug + Eq + Hash + 'a>(
    ui: &'a Ui,
    GraphStyle { symbols, crossing }: GraphStyle,
    formatter: &'a mut dyn Formatter,
) -> Box<dyn GraphLog<K> + 'a> {
    let prefix_lines: Box<dyn PrefixLineRenderer<K>> = match symbols {
        GraphSymbols::Ascii => Box::new(Ascii::new()),
        GraphSymbols::AsciiLarge => Box::new(AsciiLarge),
        GraphSymbols::Curved => Box::new(BoxDrawing::<Curved>::new()),
        GraphSymbols::Square => Box::new(BoxDrawing::<Square>::new()),
        GraphSymbols::Branch => Box::new(BranchDrawing::new()),
    };
    // Match renderdag's column widths for each style. PrefixLineRenderer does
    // not expose the column-to-character conversion through its public API.
    let columns_to_chars: fn(u64) -> u64 = match symbols {
        // The first column is only 2 characters wide.
        GraphSymbols::AsciiLarge => |columns| {
            columns
                .saturating_mul(3)
                .saturating_sub(1)
                .saturating_add(1)
        },
        _ => |columns| columns.saturating_mul(2).saturating_add(1),
    };
    let allocator = build_line_id_allocator_from_rules("parent,incoming,free", LINE_LABELS.len());
    let mut shaper = GraphRowShaper::new().with_line_id_allocator(allocator);
    let options = shaper.options_mut();
    options.min_row_height = 0;
    options.graph_row_shape_style.crossing_priority = match crossing {
        None => CrossingPriority::RendererDefault,
        Some(GraphCrossing::PreferHorizontal) => CrossingPriority::Horizontal,
        Some(GraphCrossing::PreferVertical) => CrossingPriority::Vertical,
    };
    Box::new(SaplingGraphLog {
        shaper,
        prefix_lines,
        text: PrefixLinesToText::new(),
        columns_to_chars,
        formatter,
        ui,
        branch_nodes: symbols == GraphSymbols::Branch,
    })
}
