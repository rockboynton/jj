// Copyright 2026 The Jujutsu Authors
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

//! Graph prefix lines drawn with the branch drawing symbols, U+F5D0 to U+F60D.
//!
//! These symbols are in the Private Use Area. Terminals such as Kitty and Ghostty draw them, so
//! that the lines of a graph join with no gaps. See
//! <https://github.com/kovidgoyal/kitty/pull/7681>.

use renderdag::LinkLine;
use renderdag::NodeLine;
use renderdag::PadLine;
use renderdag::pipeline::CrossingPriority;
use renderdag::pipeline::DashStyle;
use renderdag::pipeline::PrefixLineRenderer;
use renderdag::pipeline::types::GraphRowShape;
use renderdag::pipeline::types::PrefixLine;
use renderdag::pipeline::types::PrefixLineKind;
use renderdag::pipeline::types::PrefixLinePart;

use super::text_writer::TextWriter;
use super::text_writer::ancestry_lines;

// The pairs of cell edges that a line in a link line cell joins.
const UP_DOWN: u8 = 1 << 0;
const UP_LEFT: u8 = 1 << 1;
const UP_RIGHT: u8 = 1 << 2;
const LEFT_DOWN: u8 = 1 << 3;
const RIGHT_DOWN: u8 = 1 << 4;
const LEFT_RIGHT: u8 = 1 << 5;

const HORIZONTAL: &str = "\u{f5d0}";
const VERTICAL: &str = "\u{f5d1}";
const FADE_DOWN: &str = "\u{f5d4}";
/// The set has no symbols for ancestor lines, so they use the glyphs of the curved box drawing
/// style.
const ANCESTOR: &str = "\u{2577}";
const ANCESTOR_ONCE: &str = "\u{2506}";

/// The symbol for each set of joined edges.
const SYMBOLS: [(u8, &str); 26] = [
    (LEFT_RIGHT, HORIZONTAL),
    (UP_DOWN, VERTICAL),
    (RIGHT_DOWN, "\u{f5d6}"),
    (LEFT_DOWN, "\u{f5d7}"),
    (UP_RIGHT, "\u{f5d8}"),
    (UP_LEFT, "\u{f5d9}"),
    (UP_DOWN | UP_RIGHT, "\u{f5da}"),
    (UP_DOWN | RIGHT_DOWN, "\u{f5db}"),
    (UP_RIGHT | RIGHT_DOWN, "\u{f5dc}"),
    (UP_DOWN | UP_LEFT, "\u{f5dd}"),
    (UP_DOWN | LEFT_DOWN, "\u{f5de}"),
    (LEFT_DOWN | UP_LEFT, "\u{f5df}"),
    (LEFT_RIGHT | LEFT_DOWN, "\u{f5e0}"),
    (LEFT_RIGHT | RIGHT_DOWN, "\u{f5e1}"),
    (RIGHT_DOWN | LEFT_DOWN, "\u{f5e2}"),
    (LEFT_RIGHT | UP_LEFT, "\u{f5e3}"),
    (LEFT_RIGHT | UP_RIGHT, "\u{f5e4}"),
    (UP_RIGHT | UP_LEFT, "\u{f5e5}"),
    (UP_DOWN | UP_RIGHT | UP_LEFT, "\u{f5e6}"),
    (UP_DOWN | RIGHT_DOWN | LEFT_DOWN, "\u{f5e7}"),
    (LEFT_RIGHT | LEFT_DOWN | UP_LEFT, "\u{f5e8}"),
    (LEFT_RIGHT | UP_RIGHT | RIGHT_DOWN, "\u{f5e9}"),
    (UP_DOWN | RIGHT_DOWN | UP_LEFT, "\u{f5ea}"),
    (UP_DOWN | LEFT_DOWN | UP_RIGHT, "\u{f5eb}"),
    (LEFT_RIGHT | RIGHT_DOWN | UP_LEFT, "\u{f5ec}"),
    (LEFT_RIGHT | LEFT_DOWN | UP_RIGHT, "\u{f5ed}"),
];

/// Converts abstract row shapes into branch drawing graph prefix lines.
#[derive(Default)]
pub struct BranchDrawing;

impl BranchDrawing {
    /// Create a branch drawing prefix line renderer.
    pub fn new() -> Self {
        Self
    }
}

impl<N> PrefixLineRenderer<N> for BranchDrawing {
    /// Convert the next graph row shape into prefix lines.
    fn next_prefix_lines(&mut self, line: &GraphRowShape<N>) -> Vec<PrefixLine> {
        let ancestor = match line.style.dash_style {
            DashStyle::Persistent => ANCESTOR,
            DashStyle::Once => ANCESTOR_ONCE,
        };
        let writer = TextWriter::new(line);
        let mut lines = Vec::new();

        lines.push(render_pad_line(
            &writer,
            ancestor,
            line.pre_node_line(),
            PrefixLineKind::PreNode,
        ));

        // Render the nodeline
        let mut node_line = PrefixLine::new(PrefixLineKind::Node);
        for (i, entry) in line.node_line.iter().enumerate() {
            match entry {
                NodeLine::Node => {
                    node_line
                        .parts
                        .push((PrefixLinePart::NodeGlyph, Default::default()));
                    writer.write_plain(" ", &mut node_line);
                }
                NodeLine::Parent => write_cell(&writer, i, VERTICAL, &mut node_line),
                NodeLine::Ancestor => write_cell(&writer, i, ancestor, &mut node_line),
                NodeLine::Blank => write_cell(&writer, i, " ", &mut node_line),
            }
        }
        lines.push(node_line);

        lines.push(render_pad_line(
            &writer,
            ancestor,
            line.post_node_line(),
            PrefixLineKind::PostNode,
        ));

        // Render the link line
        if let Some(link_row) = line.link_line.as_ref() {
            let mut link_line = PrefixLine::new(PrefixLineKind::Link);
            for (i, cur) in link_row.iter().enumerate() {
                let up = line.node_line[i] != NodeLine::Blank;
                let down = line.ancestry_line[i] != PadLine::Blank
                    || line.term_line.as_ref().is_some_and(|term| term[i]);
                let joins = link_joins(*cur, up, down);
                // A vertical line to an ancestor, not a parent, is dashed.
                let vertical = if cur.intersects(LinkLine::VERT_PARENT) {
                    VERTICAL
                } else {
                    ancestor
                };
                // By default, the horizontal line hides the vertical line, as in the box styles.
                if line.style.crossing_priority == CrossingPriority::Vertical
                    && is_crossing(*cur, up, down)
                {
                    writer.write_junction(i, LinkLine::VERTICAL, vertical, &mut link_line);
                } else {
                    let symbol = if joins == UP_DOWN {
                        vertical
                    } else {
                        symbol(joins)
                    };
                    writer.write_junction(i, segments(*cur, joins), symbol, &mut link_line);
                }
                if joins & (UP_RIGHT | RIGHT_DOWN | LEFT_RIGHT) != 0 {
                    // The connector has its own line IDs, independent of the symbol.
                    writer.write_link(
                        i,
                        LinkLine::HORIZONTAL | LinkLine::RIGHT_FORK | LinkLine::RIGHT_MERGE,
                        HORIZONTAL,
                        &mut link_line,
                    );
                } else {
                    writer.write_plain(" ", &mut link_line);
                }
            }
            lines.push(link_line);
        }

        // Render the term line
        if let Some(term_row) = line.term_line.as_ref() {
            for term_str in [VERTICAL, FADE_DOWN] {
                let mut term_line = PrefixLine::new(PrefixLineKind::Term);
                for (i, term) in term_row.iter().enumerate() {
                    let symbol = if *term {
                        term_str
                    } else {
                        pad_symbol(ancestor, line.ancestry_line[i])
                    };
                    write_cell(&writer, i, symbol, &mut term_line);
                }
                lines.push(term_line);
            }
        }

        for (kind, columns) in ancestry_lines(line) {
            lines.push(render_pad_line(&writer, ancestor, columns, kind));
        }

        lines
    }
}

/// The segments of a link line cell that its symbol draws, for coloring.
fn segments(cell: LinkLine, joins: u8) -> LinkLine {
    let mut segments = LinkLine::empty();
    if joins & UP_DOWN != 0 {
        segments |= cell & LinkLine::VERTICAL;
    }
    if joins & UP_LEFT != 0 {
        segments |= LinkLine::LEFT_MERGE;
    }
    if joins & UP_RIGHT != 0 {
        segments |= LinkLine::RIGHT_MERGE;
    }
    if joins & LEFT_DOWN != 0 {
        segments |= LinkLine::LEFT_FORK;
    }
    if joins & RIGHT_DOWN != 0 {
        segments |= LinkLine::RIGHT_FORK;
    }
    if joins & LEFT_RIGHT != 0 {
        segments |= LinkLine::HORIZONTAL;
    }
    segments
}

/// True if a link line cell only carries a horizontal line past its column.
fn is_horizontal_run(cell: LinkLine) -> bool {
    cell.intersects(LinkLine::HORIZONTAL)
        && !cell.intersects(LinkLine::CHILD | LinkLine::ANY_FORK_OR_MERGE)
}

/// True if a vertical line crosses a horizontal line in a link line cell, with
/// no fork or merge.
fn is_crossing(cell: LinkLine, up: bool, down: bool) -> bool {
    is_horizontal_run(cell) && cell.intersects(LinkLine::VERTICAL) && up && down
}

/// Find the pairs of edges that the lines in a link line cell join.
///
/// A line runs from the node above to each of its parents below. `up` is true if a line comes in
/// from the row above, and `down` is true if a line goes on to the row below.
fn link_joins(cell: LinkLine, up: bool, down: bool) -> u8 {
    if is_horizontal_run(cell) {
        return LEFT_RIGHT;
    }
    let mut joins = 0;
    if up && down {
        joins |= UP_DOWN;
    }
    if cell.intersects(LinkLine::LEFT_MERGE) {
        joins |= UP_LEFT;
    }
    if cell.intersects(LinkLine::RIGHT_MERGE) {
        joins |= UP_RIGHT;
    }
    if cell.intersects(LinkLine::LEFT_FORK) {
        joins |= LEFT_DOWN;
    }
    if cell.intersects(LinkLine::RIGHT_FORK) {
        joins |= RIGHT_DOWN;
    }
    if cell.intersects(LinkLine::HORIZONTAL) {
        joins |= LEFT_RIGHT;
    }
    joins
}

/// The symbol for a set of joined edges.
///
/// No symbol has a vertical line that crosses a horizontal one. For such a set, the horizontal line
/// wins, as in the box drawing renderer.
fn symbol(joins: u8) -> &'static str {
    let find = |joins| SYMBOLS.iter().find(|(set, _)| *set == joins);
    match find(joins).or_else(|| find(joins & !UP_DOWN)) {
        Some((_, symbol)) => symbol,
        None if joins & LEFT_RIGHT != 0 => HORIZONTAL,
        None if joins & UP_DOWN != 0 => VERTICAL,
        None => " ",
    }
}

/// Writes a 2-character cell of `out`: `symbol`, with the line ID of column
/// `index`, and a space.
fn write_cell(writer: &TextWriter<'_>, index: usize, symbol: &str, out: &mut PrefixLine) {
    writer.write_column(index, out.kind, symbol, out);
    writer.write_plain(" ", out);
}

fn render_pad_line(
    writer: &TextWriter<'_>,
    ancestor: &str,
    line: impl IntoIterator<Item = PadLine>,
    kind: PrefixLineKind,
) -> PrefixLine {
    let mut out = PrefixLine::new(kind);
    for (i, entry) in line.into_iter().enumerate() {
        write_cell(writer, i, pad_symbol(ancestor, entry), &mut out);
    }
    out
}

/// The symbol of a padding cell. A blank cell has a space.
fn pad_symbol(ancestor: &str, line: PadLine) -> &str {
    match line {
        PadLine::Parent => VERTICAL,
        PadLine::Ancestor => ancestor,
        PadLine::Blank => " ",
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use renderdag::Ancestor;
    use renderdag::pipeline::GraphRowShaper;
    use renderdag::pipeline::PrefixLinesToText;
    use renderdag::pipeline::coloring::build_line_id_allocator_from_rules;

    use super::*;

    /// Renders the rows, and marks each piece of graph text with its line ID.
    fn render(crossing: CrossingPriority, rows: &[(&'static str, &[&'static str])]) -> String {
        let rows = rows
            .iter()
            .map(|(node, parents)| {
                let parents = parents.iter().map(|p| Ancestor::Parent(*p)).collect();
                (*node, parents)
            })
            .collect();
        render_ancestors(crossing, rows)
    }

    /// Like [`render`], but the parents can be indirect ancestors.
    fn render_ancestors(
        crossing: CrossingPriority,
        rows: Vec<(&'static str, Vec<Ancestor<&'static str>>)>,
    ) -> String {
        let allocator = build_line_id_allocator_from_rules::<&str>("parent,incoming,free", 6);
        let mut shaper = GraphRowShaper::new().with_line_id_allocator(allocator);
        shaper.options_mut().graph_row_shape_style.crossing_priority = crossing;
        let mut renderer = BranchDrawing::new();
        let mut text = PrefixLinesToText::new();
        let mut out = String::new();
        for (node, parents) in rows {
            let shape = shaper.next_row_shape(node, parents);
            let prefix = renderer.next_prefix_lines(&shape);
            text.write_next_lines(
                |piece, id| match id {
                    Some(id) => write!(out, "{{{id}:{piece}}}").unwrap(),
                    None => out.push_str(piece),
                },
                prefix,
                shape.separator_line,
                "o",
                node,
                0,
            );
        }
        out
    }

    /// Each piece of graph text carries the ID of its line.
    #[test]
    fn test_pieces_have_line_ids() {
        let rows: &[(&str, &[&str])] =
            &[("D", &["B", "C"]), ("C", &["A"]), ("B", &["A"]), ("A", &[])];
        assert_eq!(
            render(CrossingPriority::RendererDefault, rows),
            "o    D\n{0:\u{f5da}}{1:\u{f5d0}\u{f5d7}}\n{0:\u{f5d1}} o  C\no {1:\u{f5d1}}  \
             B\n{1:\u{f5db}\u{f5d0}\u{f5d9}}\no  A\n"
        );
    }

    /// The link line of D crosses the lines of F and E, in columns 1 and 2.
    const CROSSINGS: &[(&str, &[&str])] = &[
        ("G", &["A"]),
        ("F", &["B"]),
        ("E", &["C"]),
        ("D", &["A"]),
        ("C", &["A"]),
        ("B", &["A"]),
        ("A", &[]),
    ];

    /// The link line of row D, from a rendering of [`CROSSINGS`].
    fn crossing_line(crossing: CrossingPriority) -> String {
        let out = render(crossing, CROSSINGS);
        out.lines().nth(4).unwrap().to_owned()
    }

    #[test]
    fn test_crossing_horizontal() {
        // The horizontal line hides the vertical lines.
        let horizontal = "{0:\u{f5db}\u{f5d0}\u{f5d0}\u{f5d0}\u{f5d0}\u{f5d0}\u{f5d9}}";
        assert_eq!(crossing_line(CrossingPriority::RendererDefault), horizontal);
        assert_eq!(crossing_line(CrossingPriority::Horizontal), horizontal);
    }

    #[test]
    fn test_crossing_vertical() {
        // Each vertical line keeps its own ID through the crossing.
        assert_eq!(
            crossing_line(CrossingPriority::Vertical),
            "{0:\u{f5db}\u{f5d0}}{1:\u{f5d1}}{0:\u{f5d0}}{2:\u{f5d1}}{0:\u{f5d0}\u{f5d9}}"
        );
    }

    #[test]
    fn test_crossing_vertical_ancestor() {
        // The link line of D crosses an indirect line in column 1 and a direct line in column 2.
        let rows = vec![
            ("G", vec![Ancestor::Parent("A")]),
            ("F", vec![Ancestor::Ancestor("B")]),
            ("E", vec![Ancestor::Parent("C")]),
            ("D", vec![Ancestor::Parent("A")]),
            ("C", vec![Ancestor::Parent("A")]),
            ("B", vec![Ancestor::Parent("A")]),
            ("A", vec![]),
        ];
        let out = render_ancestors(CrossingPriority::Vertical, rows);
        // The indirect line in column 1 stays dashed through the crossing, and keeps its ID. The
        // direct line in column 2 stays solid.
        let link = out.lines().nth(6).unwrap();
        assert_eq!(
            link,
            "{0:\u{f5db}\u{f5d0}}{1:\u{2577}}{0:\u{f5d0}}{2:\u{f5d1}}{0:\u{f5d0}\u{f5d9}}"
        );
    }
}
