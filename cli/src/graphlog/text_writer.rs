/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Graph text helpers, adapted from sapling-renderdag 0.3.0.
//!
//! The code comes from `src/pipeline/row_shape_to_prefix_lines/text_writer.rs` and
//! `src/pipeline/row_shape_to_prefix_lines.rs` of that crate, which is MIT licensed. See
//! <https://github.com/facebook/sapling/tree/main/eden/scm/lib/renderdag>.

use renderdag::LinkLine;
use renderdag::PadLine;
use renderdag::pipeline::DashStyle;
use renderdag::pipeline::coloring::GraphRowLineIds;
use renderdag::pipeline::types::GraphRowShape;
use renderdag::pipeline::types::PrefixLine;
use renderdag::pipeline::types::PrefixLineKind;
use renderdag::pipeline::types::PrefixLinePart;

/// Writes graph text pieces with the line IDs of the row.
pub(super) struct TextWriter<'a> {
    line_ids: Option<&'a GraphRowLineIds>,
}

impl<'a> TextWriter<'a> {
    pub(super) fn new<N>(row: &'a GraphRowShape<N>) -> Self {
        Self {
            line_ids: row.line_ids.as_deref(),
        }
    }

    pub(super) fn write_column(
        &self,
        index: usize,
        kind: PrefixLineKind,
        text: &str,
        out: &mut PrefixLine,
    ) {
        let Some(ids) = self.line_ids else {
            self.write_plain(text, out);
            return;
        };
        let columns = match kind {
            PrefixLineKind::PreNode | PrefixLineKind::Node | PrefixLineKind::PostNode => {
                &ids.before_link
            }
            _ => &ids.after_link,
        };
        self.write_text(text, columns[index].iter().next(), out);
    }

    pub(super) fn write_plain(&self, text: &str, out: &mut PrefixLine) {
        append(text, None, out);
    }

    pub(super) fn write_junction(
        &self,
        index: usize,
        segments: LinkLine,
        text: &str,
        out: &mut PrefixLine,
    ) {
        self.write_links(index, segments, true, text, out);
    }

    pub(super) fn write_link(
        &self,
        index: usize,
        segments: LinkLine,
        text: &str,
        out: &mut PrefixLine,
    ) {
        self.write_links(index, segments, false, text, out);
    }

    fn write_links(
        &self,
        index: usize,
        segments: LinkLine,
        junction: bool,
        text: &str,
        out: &mut PrefixLine,
    ) {
        let Some(cells) = self.line_ids.and_then(|ids| ids.link_line.as_ref()) else {
            self.write_plain(text, out);
            return;
        };
        let cell = &cells[index];
        // A terminal character has one foreground, even at an intersection.
        if segments.intersects(LinkLine::VERTICAL)
            && let Some(id) = cell.vertical.iter().next()
        {
            self.write_text(text, Some(id), out);
            return;
        }
        // Otherwise, use the lowest ID of the drawn segments.
        let mut id = None;
        for (flag, part) in [
            (LinkLine::HORIZONTAL, &cell.horizontal),
            (LinkLine::LEFT_FORK, &cell.left_fork),
            (LinkLine::RIGHT_FORK, &cell.right_fork),
            (LinkLine::LEFT_MERGE, &cell.left_merge),
            (LinkLine::RIGHT_MERGE, &cell.right_merge),
        ] {
            if segments.intersects(flag) && (!junction || flag != LinkLine::HORIZONTAL) {
                id = id.into_iter().chain(part.iter().next()).min();
            }
        }
        if junction && id.is_none() && segments.intersects(LinkLine::HORIZONTAL) {
            // Junctions prefer their local fork and merge IDs over a passing
            // horizontal line.
            id = cell.horizontal.iter().next();
        }
        self.write_text(text, id, out);
    }

    fn write_text(&self, text: &str, id: Option<usize>, out: &mut PrefixLine) {
        if id.is_none() {
            self.write_plain(text, out);
            return;
        }
        // Padding has no line identity.
        let leading = text.len() - text.trim_start_matches(' ').len();
        let body = text[leading..].trim_end_matches(' ');
        self.write_plain(&text[..leading], out);
        append(body, id, out);
        self.write_plain(&text[leading + body.len()..], out);
    }
}

/// Appends text to the last piece if it has the same line ID.
fn append(text: &str, id: Option<usize>, out: &mut PrefixLine) {
    if text.is_empty() {
        return;
    }
    if let Some((PrefixLinePart::Text(previous), previous_id)) = out.parts.last_mut()
        && *previous_id == id
    {
        previous.push_str(text);
        return;
    }
    out.parts.push((PrefixLinePart::Text(text.to_owned()), id));
}

/// The ancestry lines after the link line, with their kinds.
pub(super) fn ancestry_lines<N>(
    row: &GraphRowShape<N>,
) -> impl Iterator<Item = (PrefixLineKind, impl Iterator<Item = PadLine> + '_)> + '_ {
    [
        row.has_indirect_ancestor()
            .then_some(PrefixLineKind::Ancestry),
        Some(PrefixLineKind::PostAncestry),
    ]
    .into_iter()
    .flatten()
    .map(move |kind| {
        let solid = kind == PrefixLineKind::PostAncestry && row.style.dash_style == DashStyle::Once;
        let columns = row.ancestry_line.iter().copied().map(move |pad| {
            if solid && pad == PadLine::Ancestor {
                PadLine::Parent
            } else {
                pad
            }
        });
        (kind, columns)
    })
}
