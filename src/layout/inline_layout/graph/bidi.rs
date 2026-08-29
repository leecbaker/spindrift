use super::*;

/// UAX #9 controls that must be virtually restored around a selected
/// soft-wrapped line.
///
/// The controls are UAX #9 input only: they are never measured, painted, or
/// exposed for extraction. They keep a CSS bidi scope intact while UAX #9
/// resolves one formatted line at a time. This applies equally to controls
/// authored in text and controls synthesized for CSS `unicode-bidi`.
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi> and
/// <https://www.unicode.org/reports/tr9/#Explicit_Levels_and_Directions>.
#[derive(Debug, Clone, Default)]
pub(in crate::layout) struct BidiLineScopeContinuations {
    /// A non-painting parent-paragraph directional context restored before a
    /// wrapped isolate. This is separate from the scope control itself: CSS
    /// resolves an isolate as a U+FFFC-like neutral in its parent paragraph,
    /// so its edge neutrals retain the nearest parent strong direction even
    /// when that text was selected onto another line.
    pub(in crate::layout) prefix_parent_context: String,
    pub(in crate::layout) prefix: String,
    pub(in crate::layout) suffix: String,
    /// Virtual bidi-only text placed immediately after selected source text.
    /// This is distinct from `suffix`: CSS scope terminators must remain after
    /// the line-edge context they balance.
    pub(in crate::layout) trailing_line_edge_context: String,
    /// See `prefix_parent_context`.
    pub(in crate::layout) suffix_parent_context: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BidiControlScope {
    start: char,
    end: char,
    is_isolate: bool,
}

impl InlineOpportunityGraph {
    /// Return cloneable inline scopes that are lexically open immediately
    /// before `position`. The stack retains non-clone scopes as empty entries
    /// while scanning so a nested clone scope is paired with its real source
    /// end rather than an unrelated outer edge.
    fn clone_scopes_before(
        &self,
        position: InlineGraphPosition,
    ) -> Vec<InlineFragmentContinuation> {
        let mut scopes = Vec::<Option<InlineFragmentContinuation>>::new();
        for run in self.runs.iter().take(position.run_index) {
            let InlineLineItem::Atom(atom) = &run.item else {
                continue;
            };
            let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) = atom.content()
            else {
                continue;
            };
            // A positioned bidi isolate carries a separate zero-advance
            // marker *inside* its authored isolate controls. It establishes
            // containing-block identity, but it is not a box-decoration
            // boundary and must not be replayed outside the virtual controls
            // added to a continuation line.
            if edge.is_positioning_marker() {
                continue;
            }
            match edge.logical_edge {
                InlineLogicalEdge::Start => {
                    scopes.push(InlineFragmentContinuation::from_source_start(atom));
                }
                InlineLogicalEdge::End => {
                    scopes.pop();
                }
            }
        }
        scopes.into_iter().flatten().collect()
    }

    /// Add the synthetic edge atoms required by `box-decoration-break: clone`
    /// to one selected graph range. Source-owned edges remain in the range;
    /// only scopes that were already open at the leading/trailing selected
    /// boundary receive continuation chrome.
    pub(super) fn insert_clone_continuation_edges(
        &self,
        range: InlineGraphRange,
        items: &mut Vec<MeasuredInlineItem>,
    ) {
        let leading = self.clone_scopes_before(range.start);
        let trailing = self.clone_scopes_before(range.end);
        if leading.is_empty() && trailing.is_empty() {
            return;
        }
        let mut continued = Vec::with_capacity(leading.len() + items.len() + trailing.len());
        continued.extend(
            leading
                .iter()
                .map(InlineFragmentContinuation::start_measured_item),
        );
        continued.append(items);
        continued.extend(
            trailing
                .iter()
                .rev()
                .map(InlineFragmentContinuation::end_measured_item),
        );
        mark_clone_continuation_fragment_edges(&mut continued, leading.len(), trailing.len());
        *items = continued;
    }

    /// Return virtual UAX #9 controls needed to balance one selected line.
    ///
    /// An isolate is one U+FFFC-like object to its containing bidi paragraph,
    /// even when line breaking selects only a middle fragment of the isolate.
    /// Reopening the scopes active before the selected range and closing
    /// scopes still active after it gives UAX #9 that same scoped input
    /// without adding glyphs or source text to the line.
    pub(in crate::layout) fn bidi_scope_continuations_for_range(
        &self,
        range: InlineGraphRange,
    ) -> BidiLineScopeContinuations {
        let scopes_before_start = self.bidi_control_scopes_before(range.start);
        let scopes_before_end = self.bidi_control_scopes_before(range.end);
        BidiLineScopeContinuations {
            prefix_parent_context: scopes_before_start
                .iter()
                .any(|scope| scope.is_isolate)
                .then(|| self.parent_direction_before(range.start))
                .flatten()
                .map(bidi_prefix_parent_context_control)
                .unwrap_or_default()
                .to_owned(),
            prefix: scopes_before_start
                .iter()
                .map(|scope| scope.start)
                .collect(),
            suffix: scopes_before_end
                .iter()
                .rev()
                .map(|scope| scope.end)
                .collect(),
            trailing_line_edge_context: String::new(),
            suffix_parent_context: scopes_before_end
                .iter()
                .any(|scope| scope.is_isolate)
                .then(|| {
                    self.parent_direction_after(range.end)
                        .or_else(|| self.parent_direction_before(range.end))
                })
                .flatten()
                .map(bidi_suffix_parent_context_control)
                .unwrap_or_default()
                .to_owned(),
        }
    }

    fn parent_direction_before(&self, position: InlineGraphPosition) -> Option<Direction> {
        let mut scopes = Vec::new();
        let mut direction = None;
        for (run_index, run) in self.runs.iter().enumerate() {
            if run_index > position.run_index {
                break;
            }
            let InlineLineItem::Fragment(fragment) = &run.item else {
                continue;
            };
            let end = if run_index == position.run_index {
                position.byte_offset.min(fragment.text().len())
            } else {
                fragment.text().len()
            };
            let Some(text) = fragment.text().get(..end) else {
                continue;
            };
            if scopes.is_empty()
                && let Some(found) = plaintext_direction_for_text(text)
            {
                direction = Some(found);
            }
            update_bidi_control_scope_stack(&mut scopes, text);
        }
        direction
    }

    fn parent_direction_after(&self, position: InlineGraphPosition) -> Option<Direction> {
        let mut scopes = self.bidi_control_scopes_before(position);
        for (run_index, run) in self.runs.iter().enumerate().skip(position.run_index) {
            let InlineLineItem::Fragment(fragment) = &run.item else {
                continue;
            };
            let start = if run_index == position.run_index {
                position.byte_offset.min(fragment.text().len())
            } else {
                0
            };
            let Some(text) = fragment.text().get(start..) else {
                continue;
            };
            if scopes.is_empty()
                && let Some(direction) = plaintext_direction_for_text(text)
            {
                return Some(direction);
            }
            update_bidi_control_scope_stack(&mut scopes, text);
        }
        None
    }

    fn bidi_control_scopes_before(&self, position: InlineGraphPosition) -> Vec<BidiControlScope> {
        let mut scopes = Vec::new();
        for (run_index, run) in self.runs.iter().enumerate() {
            if run_index > position.run_index {
                break;
            }
            let InlineLineItem::Fragment(fragment) = &run.item else {
                continue;
            };
            let end = if run_index == position.run_index {
                position.byte_offset.min(fragment.text().len())
            } else {
                fragment.text().len()
            };
            if let Some(text) = fragment.text().get(..end) {
                update_bidi_control_scope_stack(&mut scopes, text);
            }
        }
        scopes
    }
}

/// Apply the UAX #9 explicit-formatting controls in `text` to an open scope
/// stack. The stack preserves only the information needed to replay a sliced
/// line's control prefix/suffix; paragraph-level level resolution remains the
/// responsibility of the bidi shaper.
///
/// `PDI` terminates its isolate and any embeddings nested inside it, while
/// `PDF` terminates only an immediately active embedding or override. Invalid
/// unmatched terminators are ignored, as required by UAX #9 X7/X8.
/// <https://www.unicode.org/reports/tr9/#Explicit_Levels_and_Directions>.
fn update_bidi_control_scope_stack(scopes: &mut Vec<BidiControlScope>, text: &str) {
    for character in text.chars() {
        let Some(scope) = bidi_control_scope_start(character) else {
            match character {
                '\u{202c}' => {
                    if scopes.last().is_some_and(|scope| !scope.is_isolate) {
                        scopes.pop();
                    }
                }
                '\u{2069}' => {
                    if let Some(isolate_index) = scopes.iter().rposition(|scope| scope.is_isolate) {
                        scopes.truncate(isolate_index);
                    }
                }
                _ => {}
            }
            continue;
        };
        scopes.push(scope);
    }
}

fn bidi_control_scope_start(character: char) -> Option<BidiControlScope> {
    let (end, is_isolate) = match character {
        '\u{202a}' | '\u{202b}' | '\u{202d}' | '\u{202e}' => ('\u{202c}', false),
        '\u{2066}' | '\u{2067}' | '\u{2068}' => ('\u{2069}', true),
        _ => return None,
    };
    Some(BidiControlScope {
        start: character,
        end,
        is_isolate,
    })
}

fn bidi_prefix_parent_context_control(direction: Direction) -> &'static str {
    match direction {
        Direction::Ltr => "\u{200e}",
        Direction::Rtl => "\u{200f}",
    }
}

fn bidi_suffix_parent_context_control(direction: Direction) -> &'static str {
    match direction {
        Direction::Ltr => "\u{200e}",
        Direction::Rtl => "\u{200f}",
    }
}
