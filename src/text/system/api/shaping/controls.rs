use super::super::*;

pub(crate) fn span_boundary_needs_join_control(left: &str, right: &str) -> bool {
    if left
        .chars()
        .next_back()
        .is_some_and(character_is_join_control)
        || right.chars().next().is_some_and(character_is_join_control)
    {
        return false;
    }
    let Some(left) = left
        .chars()
        .rev()
        .find(|character| !character_is_join_control(*character))
    else {
        return false;
    };
    let Some(right) = right
        .chars()
        .find(|character| !character_is_join_control(*character))
    else {
        return false;
    };
    cursive_boundary_needs_context(left, right)
}

/// Provenance for characters that exist in the shaping buffer but are not
/// necessarily authored CSS text.
///
/// Keeping this distinction typed prevents an internal boundary-control glyph
/// from being confused with authored source text during PDF conversion:
/// <https://drafts.csswg.org/css-text-3/#boundary-shaping>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::text) enum ShapingContextKind {
    AuthoredJoinControl,
    SyntheticBoundaryJoinControl,
    SyntheticEdgeContext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::text) struct ShapingContextRange {
    range: Range<usize>,
    kind: ShapingContextKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::text) struct ShapingContextMap {
    ranges: Vec<ShapingContextRange>,
}

impl ShapingContextMap {
    #[cfg(test)]
    pub(in crate::text) fn from_synthetic_ranges(ranges: &[Range<usize>]) -> Self {
        Self {
            ranges: ranges
                .iter()
                .cloned()
                .map(|range| ShapingContextRange {
                    range,
                    kind: ShapingContextKind::SyntheticBoundaryJoinControl,
                })
                .collect(),
        }
    }

    fn push(&mut self, range: Range<usize>, kind: ShapingContextKind) {
        self.ranges.push(ShapingContextRange { range, kind });
    }

    fn shift_after_insertion(&mut self, index: usize, amount: usize) {
        for context in &mut self.ranges {
            if context.range.start >= index {
                context.range.start += amount;
                context.range.end += amount;
            } else if context.range.end >= index {
                context.range.end += amount;
            }
        }
    }

    pub(in crate::text) fn add_authored_join_controls(&mut self, text: &str) {
        for (start, character) in text.char_indices() {
            let end = start + character.len_utf8();
            if character_is_join_control(character)
                && !self.is_synthetic_at(start)
                && !self
                    .ranges
                    .iter()
                    .any(|context| context.range.start == start && context.range.end == end)
            {
                self.push(start..end, ShapingContextKind::AuthoredJoinControl);
            }
        }
    }

    pub(in crate::text) fn is_synthetic_at(&self, index: usize) -> bool {
        self.ranges.iter().any(|context| {
            context.range.contains(&index)
                && matches!(
                    context.kind,
                    ShapingContextKind::SyntheticBoundaryJoinControl
                        | ShapingContextKind::SyntheticEdgeContext
                )
        })
    }

    fn is_synthetic_only(&self, range: Range<usize>) -> bool {
        range.start < range.end
            && self.ranges.iter().any(|context| {
                context.range.start <= range.start
                    && range.end <= context.range.end
                    && matches!(
                        context.kind,
                        ShapingContextKind::SyntheticBoundaryJoinControl
                            | ShapingContextKind::SyntheticEdgeContext
                    )
            })
    }

    pub(in crate::text) fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }
}

/// Append a shaping-only ZWJ and remember its byte range for output cleanup.
///
/// CSS Text shaping may need join controls that are not present in the DOM
/// text. Tracking synthetic controls separately preserves the original text
/// for PDF extraction while still giving OpenType shaping the required
/// joining context:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
pub(in crate::text) fn push_synthetic_join_control(
    text: &mut String,
    contexts: &mut ShapingContextMap,
) {
    let start = text.len();
    text.push('\u{200d}');
    contexts.push(
        start..text.len(),
        ShapingContextKind::SyntheticBoundaryJoinControl,
    );
}

pub(in crate::text) fn text_needs_edge_join_context(text: &str) -> bool {
    text_needs_leading_join_context(text) || text_needs_trailing_join_context(text)
}

pub(in crate::text) fn text_needs_leading_join_context(text: &str) -> bool {
    let mut characters = text.chars();
    if characters.next() != Some('\u{200d}') {
        return false;
    }
    characters
        .find(|character| !character_is_join_control(*character))
        .is_some_and(|character| {
            character_can_join_preceding(character)
                && character_supports_arabic_tatweel_edge_context(character)
        })
}

pub(in crate::text) fn text_needs_trailing_join_context(text: &str) -> bool {
    let mut characters = text.chars().rev();
    if characters.next() != Some('\u{200d}') {
        return false;
    }
    characters
        .find(|character| !character_is_join_control(*character))
        .is_some_and(|character| {
            character_can_join_following(character)
                && character_supports_arabic_tatweel_edge_context(character)
        })
}

/// Add shaping-only tatweel at run edges requested by explicit ZWJ.
///
/// U+200D at the start or end of an isolated shaping run asks the shaper to
/// form a connection to a neighboring joining context. Some shaping backends
/// do not apply that edge context without a concrete joining neighbor, so the
/// renderer supplies U+0640 ARABIC TATWEEL as shaping-only context and removes
/// it from emitted glyph text:
/// <https://www.w3.org/TR/css-text-3/#text-encoding> and
/// <https://www.w3.org/TR/alreq/#h_joining_enforcement>.
pub(in crate::text) fn push_edge_join_context<T>(
    text: &mut String,
    ranges: &mut Vec<(Range<usize>, &T)>,
    contexts: &mut ShapingContextMap,
) {
    let mut insertions = Vec::new();
    for (range, _) in ranges.iter() {
        let Some(slice) = text.get(range.clone()) else {
            continue;
        };
        if let Some(index) = leading_join_context_insertion_index(slice) {
            insertions.push(range.start + index);
        } else if text[..range.start].ends_with('\u{200d}')
            && slice.chars().next().is_some_and(|character| {
                character_can_join_preceding(character)
                    && character_supports_arabic_tatweel_edge_context(character)
            })
        {
            // A joiner can be owned by a separately styled span (including a
            // fallback font face selected solely for U+200D).  The adjacent
            // Arabic span still needs a concrete joining neighbor; retain the
            // authored joiner and add only the shaping-only tatweel context at
            // the styled boundary.
            // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
            // <https://www.w3.org/TR/alreq/#h_joining-enforcement>
            insertions.push(range.start);
        }
        if let Some(index) = trailing_join_context_insertion_index(slice) {
            insertions.push(range.start + index);
        } else if text[range.end..].starts_with('\u{200d}')
            && slice.chars().next_back().is_some_and(|character| {
                character_can_join_following(character)
                    && character_supports_arabic_tatweel_edge_context(character)
            })
        {
            insertions.push(range.end);
        }
    }
    insertions.sort_unstable();
    insertions.dedup();
    for index in insertions.into_iter().rev() {
        insert_synthetic_join_context(text, ranges, contexts, index);
    }
}

pub(in crate::text) fn range_is_synthetic_only(
    range: Range<usize>,
    contexts: &ShapingContextMap,
) -> bool {
    contexts.is_synthetic_only(range)
}

pub(in crate::text) fn leading_join_context_insertion_index(text: &str) -> Option<usize> {
    if !text.starts_with('\u{200d}') {
        return None;
    }
    text.char_indices()
        .find(|(_, character)| !character_is_join_control(*character))
        .and_then(|(index, character)| {
            (character_can_join_preceding(character)
                && character_supports_arabic_tatweel_edge_context(character))
            .then_some(index)
        })
}

pub(in crate::text) fn trailing_join_context_insertion_index(text: &str) -> Option<usize> {
    if !text.ends_with('\u{200d}') {
        return None;
    }
    text.char_indices()
        .rev()
        .find(|(_, character)| !character_is_join_control(*character))
        .and_then(|(index, character)| {
            (character_can_join_following(character)
                && character_supports_arabic_tatweel_edge_context(character))
            .then_some(index + character.len_utf8())
        })
}

pub(in crate::text) fn insert_synthetic_join_context<T>(
    text: &mut String,
    ranges: &mut [(Range<usize>, &T)],
    contexts: &mut ShapingContextMap,
    index: usize,
) {
    let context_len = '\u{0640}'.len_utf8();
    text.insert(index, '\u{0640}');
    for (range, _) in ranges.iter_mut() {
        if range.start >= index {
            range.start += context_len;
            range.end += context_len;
        } else if range.end >= index {
            range.end += context_len;
        }
    }
    contexts.shift_after_insertion(index, context_len);
    contexts.push(
        index..index + context_len,
        ShapingContextKind::SyntheticEdgeContext,
    );
}

/// Remove shaping-only join controls from emitted text content.
///
/// PDF ToUnicode data should reflect the document text, not internal shaping
/// controls inserted to satisfy CSS Text boundary shaping:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and ISO 32000-2
/// section 9.10.3.
pub(in crate::text) fn text_without_synthetic_join_controls(
    text: &str,
    range: Range<usize>,
    contexts: &ShapingContextMap,
) -> String {
    let Some(slice) = text.get(range.clone()) else {
        return String::new();
    };
    slice
        .char_indices()
        .filter_map(|(offset, character)| {
            let index = range.start + offset;
            (!contexts.is_synthetic_at(index)).then_some(character)
        })
        .collect()
}

/// Remove shaping-only join-control glyph records from fallback-shaped output.
///
/// The fallback shaper maps one input character to one glyph, so synthetic ZWJ
/// code points can be dropped without changing visible glyph advances:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
pub(in crate::text) fn glyphs_without_synthetic_join_controls(
    glyphs: Vec<RenderedGlyph>,
    raw_text: &str,
    run_start: usize,
    contexts: &ShapingContextMap,
) -> Vec<RenderedGlyph> {
    let mut glyphs = glyphs.into_iter();
    let mut output = raw_text
        .char_indices()
        .filter_map(|(offset, character)| {
            let mut glyph = glyphs.next()?;
            let index = run_start + offset;
            if contexts.is_synthetic_at(index) || character_is_join_control(character) {
                None
            } else {
                glyph.unicode = character.to_string();
                Some(glyph)
            }
        })
        .collect::<Vec<_>>();
    output.extend(glyphs);
    output
}

/// An advance reported by the shaping backend, in its inline-axis coordinate
/// system. It must not be added directly to CSS layout widths.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(in crate::text) struct ShaperInlineAdvance(f32);

impl ShaperInlineAdvance {
    pub(in crate::text) fn from_parley(value: f32) -> Self {
        Self(value)
    }

    pub(in crate::text) fn points(self) -> f32 {
        self.0
    }

    pub(in crate::text) fn is_zero(self) -> bool {
        self.0 == 0.0
    }
}

/// A visual position reported by the shaping backend before conversion to
/// physical paint coordinates.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(in crate::text) struct ShaperVisualInlinePosition(f32);

impl ShaperVisualInlinePosition {
    pub(in crate::text) fn from_parley(value: f32) -> Self {
        Self(value)
    }

    pub(in crate::text) fn points(self) -> f32 {
        self.0
    }
}

/// A CSS layout inline advance. Control-only shaping runs always convert to
/// this zero value, regardless of a fallback font's nominal advance.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(in crate::text) struct LayoutInlineAdvance(f32);

impl LayoutInlineAdvance {
    pub(in crate::text) fn zero() -> Self {
        Self(0.0)
    }

    pub(in crate::text) fn points(self) -> f32 {
        self.0
    }
}

/// A control-only run that participated in shaping but must not contribute
/// visible fallback geometry.
///
/// Font fallback may select a face solely to map a ZWJ, ZWNJ, or another
/// default-ignorable control. The control remains part of the shaping stream,
/// but CSS Text does not give it visual advance. Parley has already included
/// that fallback run's advance in the following runs' visual offsets, so
/// conversion removes the advance from those offsets when the run is omitted.
/// <https://www.w3.org/TR/css-text-3/#text-encoding>
#[derive(Debug, Clone, PartialEq)]
pub(in crate::text) struct SourceOnlyRun {
    pub(in crate::text) visual_position: ShaperVisualInlinePosition,
    pub(in crate::text) shaper_advance: ShaperInlineAdvance,
    /// Authored controls remain in extraction text even though this fallback
    /// run produces no paintable glyphs.
    pub(in crate::text) text: Rc<str>,
}

impl SourceOnlyRun {
    pub(in crate::text) fn layout_inline_advance(&self) -> LayoutInlineAdvance {
        LayoutInlineAdvance::zero()
    }
}

/// Return whether a complete shaping run consists only of default-ignorable
/// controls.
pub(in crate::text) fn text_is_join_control_only(text: &str) -> bool {
    !text.is_empty() && text.chars().all(character_is_join_control)
}

/// How a fallback run containing a default-ignorable shaping control converts
/// to PDF glyph payload.
///
/// Default-ignorable controls remain in Parley's input. Conversion may omit a
/// control-only fallback run, or safely re-home one simple visible scalar to
/// its selected CSS face. All other clusters retain Parley's glyph result so
/// complex-script substitutions and positioning cannot be weakened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::text) enum ControlFallbackCluster {
    DropControlOnly,
    RehomeSimpleVisibleFragment { character: char },
    PreserveParleyGlyphs,
}

/// Inputs needed to re-home one safe control fallback fragment.
#[derive(Debug, Clone)]
pub(in crate::text) struct ControlFallbackRehomeRequest {
    pub(in crate::text) character: char,
    pub(in crate::text) fallback_font_id: usize,
    pub(in crate::text) text: Rc<str>,
    pub(in crate::text) font_size: f32,
    pub(in crate::text) visual_position: ShaperVisualInlinePosition,
    pub(in crate::text) shaper_advance: ShaperInlineAdvance,
    pub(in crate::text) source_range: Option<Range<usize>>,
}

/// Classify a Parley fallback run that may contain ZWNJ/ZWJ.
///
/// Re-homing is intentionally proof-based: one visible source scalar, at
/// least one join control, an unpositioned Parley glyph sequence, and a script
/// whose shaping does not use cursive or Indic-style contextual forms. Any
/// ambiguous cluster keeps its original Parley glyph payload.
pub(in crate::text) fn classify_control_fallback_cluster(
    text: &str,
    has_positioned_glyph: bool,
) -> ControlFallbackCluster {
    if text_is_join_control_only(text) {
        return ControlFallbackCluster::DropControlOnly;
    }
    if has_positioned_glyph || !text.chars().any(character_is_join_control) {
        return ControlFallbackCluster::PreserveParleyGlyphs;
    }
    let mut visible = text
        .chars()
        .filter(|character| !character_is_default_ignorable_code_point(*character));
    let Some(character) = visible.next() else {
        return ControlFallbackCluster::PreserveParleyGlyphs;
    };
    if visible.next().is_none() && character_has_simple_shaping(character) {
        ControlFallbackCluster::RehomeSimpleVisibleFragment { character }
    } else {
        ControlFallbackCluster::PreserveParleyGlyphs
    }
}

fn character_has_simple_shaping(character: char) -> bool {
    if character_has_cursive_shaping_behavior(character) {
        return false;
    }
    let script = CodePointMapData::<IcuScript>::new().get(character);
    let Some(script) = PropertyNamesShort::<IcuScript>::new().get_locale_script(script) else {
        return false;
    };
    let tag = script.into_raw();
    tag == *b"Latn" || tag == *b"Grek" || tag == *b"Cyrl" || tag == *b"Armn" || tag == *b"Geor"
}

/// Remove omitted default-ignorable fallback advances from a visual run
/// origin.
///
/// Parley exposes run origins in physical visual order. Removing a positive
/// advance therefore shifts only origins physically after the omitted run,
/// independent of the run's logical bidi direction.
pub(in crate::text) fn corrected_visual_inline_position(
    position: ShaperVisualInlinePosition,
    dropped_runs: &[SourceOnlyRun],
) -> ShaperVisualInlinePosition {
    debug_assert!(
        dropped_runs
            .iter()
            .all(|run| run.layout_inline_advance().points() == 0.0)
    );
    ShaperVisualInlinePosition(
        position.0
            - dropped_runs
                .iter()
                .filter(|dropped| dropped.visual_position < position)
                .map(|dropped| dropped.shaper_advance.points())
                .sum::<f32>(),
    )
}

/// Stitch a re-homed control fallback fragment to its immediately following
/// compatible visual run.
///
/// The control remains in the combined source text, while the glyph stream is
/// the selected-face stream that would have been produced without the fallback
/// control glyph. Stitching is limited to increasing physical origins; RTL and
/// any non-adjacent visual arrangement retain independent runs.
pub(in crate::text) fn stitch_rehomed_control_fallback_run(
    runs: &mut Vec<ShapedGlyphRun>,
    index: usize,
) -> bool {
    let Some(next_index) = index.checked_add(1).filter(|index| *index < runs.len()) else {
        return false;
    };
    let previous = &runs[index];
    let next = &runs[next_index];
    if next.x_offset < previous.x_offset
        || previous.font_id != next.font_id
        || previous.font_size != next.font_size
        || previous.font_palette != next.font_palette
    {
        return false;
    }
    let next = runs.remove(next_index);
    let previous = &mut runs[index];
    previous.text = format!("{}{}", previous.text, next.text).into();
    previous.glyphs.extend(next.glyphs);
    previous
        .glyph_source_ranges
        .extend(next.glyph_source_ranges);
    true
}

/// Retain an omitted authored join-control run in adjacent visual text.
///
/// A default-ignorable fallback run contributes neither glyphs nor advance,
/// but dropping its source text would change copy/paste and accessibility
/// text. When its neighboring visual runs use the same text presentation,
/// merge all three into one run; otherwise attach the control to the prior
/// run so extraction still preserves logical text without changing paint.
pub(in crate::text) fn stitch_dropped_join_control_runs(
    runs: &mut Vec<ShapedGlyphRun>,
    dropped_runs: &[SourceOnlyRun],
) {
    for dropped in dropped_runs {
        if !dropped.text.chars().any(character_is_join_control) {
            continue;
        }
        let Some(previous_index) = runs
            .iter()
            .enumerate()
            .filter(|(_, run)| run.x_offset <= dropped.visual_position.points())
            .map(|(index, _)| index)
            .next_back()
        else {
            continue;
        };
        let next_index = (previous_index + 1..runs.len())
            .find(|&index| runs[index].x_offset >= dropped.visual_position.points());
        let mut combined_text = String::from(runs[previous_index].text.as_ref());
        combined_text.push_str(&dropped.text);
        let can_merge_next = next_index.is_some_and(|index| {
            let previous = &runs[previous_index];
            let next = &runs[index];
            previous.font_id == next.font_id
                && previous.font_size == next.font_size
                && previous.font_palette == next.font_palette
                && previous.y_offset == next.y_offset
                && previous.text_matrix == next.text_matrix
        });
        if can_merge_next {
            let next = runs.remove(next_index.expect("checked above"));
            combined_text.push_str(&next.text);
            let previous = &mut runs[previous_index];
            previous.text = combined_text.into();
            previous.glyphs.extend(next.glyphs);
            previous
                .glyph_source_ranges
                .extend(next.glyph_source_ranges);
        } else {
            runs[previous_index].text = combined_text.into();
        }
    }
}

pub(in crate::text) fn apply_synthetic_position_fallback(
    glyphs: &mut [RenderedGlyph],
    font_size: &mut f32,
    style: &ComputedStyle,
    face: &ttf_parser::Face<'_>,
    text: &str,
) {
    if !style.font_synthesis.position {
        return;
    }
    let (scale, shift) = match style.font_variant_position {
        FontVariantPosition::Sub => (0.65, -*font_size * 0.2),
        FontVariantPosition::Super => (0.65, *font_size * 0.35),
        FontVariantPosition::Normal => return,
    };
    if opentype_position_feature_substituted(glyphs, face, text) {
        return;
    }
    *font_size *= scale;
    for glyph in glyphs {
        glyph.x_advance *= scale;
        glyph.nominal_x_advance *= scale;
        glyph.x_offset *= scale;
        glyph.y_offset = glyph.y_offset * scale + shift;
    }
}

pub(in crate::text) fn opentype_position_feature_substituted(
    glyphs: &[RenderedGlyph],
    face: &ttf_parser::Face<'_>,
    text: &str,
) -> bool {
    let mut visible_glyphs = glyphs
        .iter()
        .filter(|glyph| !glyph.unicode.is_empty())
        .filter(|glyph| {
            glyph
                .unicode
                .chars()
                .any(|character| !character_is_default_ignorable_code_point(character))
        });
    let mut visible_characters = text
        .chars()
        .filter(|character| !character_is_default_ignorable_code_point(*character));

    // CSS Fonts requires an all-or-nothing choice for a super/subscript run:
    // if the requested OpenType feature cannot provide alternates for every
    // character, synthesize the whole run instead.  In particular, treating a
    // single substituted digit as sufficient would mix Lato's `sups` glyphs
    // with ordinary spaces and punctuation.
    // <https://drafts.csswg.org/css-fonts-4/#font-variant-position-prop>
    let mut has_visible_character = false;
    loop {
        match (visible_characters.next(), visible_glyphs.next()) {
            (Some(character), Some(glyph)) => {
                has_visible_character = true;
                let Some(nominal) = face.glyph_index(character) else {
                    return false;
                };
                if Some(nominal.0) == glyph.painted_id() {
                    return false;
                }
            }
            (None, None) => return has_visible_character,
            _ => return false,
        }
    }
}

/// Return whether a shaped glyph cluster represents only default-ignorable code points.
///
/// CSS text shaping must preserve controls such as ZWJ/ZWNJ and variation
/// selectors in shaping input, while PDF painting must not emit visible
/// fallback glyphs for clusters made only from Unicode default-ignorable code
/// points:
/// <https://www.w3.org/TR/css-text-3/#text-encoding>,
/// <https://www.unicode.org/reports/tr44/#Default_Ignorable_Code_Point>, and
/// ISO 32000-2 section 9.10.3.
pub(in crate::text) fn cluster_is_default_ignorable_only(
    raw_text: &str,
    emitted_text: &str,
) -> bool {
    !raw_text.is_empty()
        && raw_text
            .chars()
            .all(character_is_default_ignorable_code_point)
        && (emitted_text.is_empty()
            || emitted_text
                .chars()
                .all(character_is_default_ignorable_code_point))
}

/// Return whether an empty glyph is a non-painting shaping artifact.
///
/// Join controls can become a font-internal space glyph: its used advance is
/// zero, but its nominal font advance is non-zero. It must not enter the PDF
/// stream because it has no ToUnicode value. A genuine positioned mark also
/// has zero used advance, but its nominal advance is zero, so it remains
/// paintable. This identifies the artifact without treating all
/// default-ignorable source ranges as disposable.
///
/// <https://www.w3.org/TR/css-text-3/#text-encoding> and ISO 32000-2:2020,
/// 9.10.3.
pub(in crate::text) fn glyph_is_non_painting_shaping_artifact(
    face: &ttf_parser::Face<'_>,
    glyph_id: u16,
    used_advance: f32,
    unicode: &str,
) -> bool {
    unicode.is_empty()
        && used_advance == 0.0
        && face
            .glyph_hor_advance(ttf_parser::GlyphId(glyph_id))
            .is_some_and(|advance| advance != 0)
}

/// Return whether an empty-Unicode glyph in a join-control cluster is the
/// fallback glyph for the control itself rather than a visible contextual
/// glyph. CID 0 is the common fallback representation; the nominal joiner
/// glyph IDs cover fonts that map the controls explicitly.
pub(in crate::text) fn glyph_is_join_control_artifact(
    face: &ttf_parser::Face<'_>,
    glyph_id: u16,
    unicode: &str,
    cluster_text: &str,
) -> bool {
    unicode.is_empty()
        && cluster_text.chars().any(character_is_join_control)
        && (glyph_id == 0
            || ['\u{200c}', '\u{200d}'].into_iter().any(|character| {
                face.glyph_index(character)
                    .is_some_and(|nominal| nominal.0 == glyph_id)
            }))
}

pub(in crate::text) fn default_ignorable_cluster_has_shaping_glyph(
    face: &ttf_parser::Face<'_>,
    cluster_text: &str,
    emitted_cluster_text: &str,
    glyphs: impl IntoIterator<Item = (u16, f32)>,
) -> bool {
    if !cluster_text.chars().any(character_is_join_control) {
        return cluster_text
            .chars()
            .any(|character| !character_is_default_ignorable_code_point(character))
            && glyphs.into_iter().any(|(glyph_id, advance)| {
                advance != 0.0
                    && !emitted_cluster_text.chars().any(|character| {
                        face.glyph_index(character)
                            .is_some_and(|nominal| nominal.0 == glyph_id)
                    })
            });
    }
    let nominal_cluster_glyphs = cluster_text
        .chars()
        .filter_map(|character| face.glyph_index(character).map(|glyph| glyph.0))
        .collect::<Vec<_>>();
    if cluster_text
        .chars()
        .any(|character| !character_is_default_ignorable_code_point(character))
    {
        return glyphs.into_iter().any(|(glyph_id, advance)| {
            advance != 0.0
                && !emitted_cluster_text.chars().any(|character| {
                    face.glyph_index(character)
                        .is_some_and(|nominal| nominal.0 == glyph_id)
                })
        });
    }

    // A complex shaper may assign the visible contextual glyph to the
    // cluster containing an edge join control. Such a cluster is still
    // source-default-ignorable, but dropping it wholesale would discard the
    // neighboring letter's glyph. Keep nonzero glyphs that are neither the
    // nominal control glyph nor CID 0; the per-glyph artifact checks below
    // remove the control/fallback glyphs while retaining the contextual one.
    glyphs.into_iter().any(|(glyph_id, advance)| {
        advance != 0.0 && glyph_id != 0 && !nominal_cluster_glyphs.contains(&glyph_id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::paint::text::RenderedTextMatrix;
    #[test]
    fn join_control_only_text_is_distinguished_from_other_ignorables() {
        assert!(text_is_join_control_only("\u{200c}\u{200d}"));
        assert!(!text_is_join_control_only("f\u{200c}i"));
        assert!(!text_is_join_control_only("\u{fe0f}"));
        assert!(!text_is_join_control_only("\u{00ad}"));
        assert!(!text_is_join_control_only(""));
    }

    #[test]
    fn control_fallback_cluster_classification_preserves_ambiguous_shaping() {
        assert_eq!(
            classify_control_fallback_cluster("\u{200c}\u{200d}", false),
            ControlFallbackCluster::DropControlOnly,
        );
        assert_eq!(
            classify_control_fallback_cluster("f\u{200c}", false),
            ControlFallbackCluster::RehomeSimpleVisibleFragment { character: 'f' },
        );
        assert_eq!(
            classify_control_fallback_cluster("f\u{200c}", true),
            ControlFallbackCluster::PreserveParleyGlyphs,
        );
        assert_eq!(
            classify_control_fallback_cluster("fi\u{200c}", false),
            ControlFallbackCluster::PreserveParleyGlyphs,
        );
        assert_eq!(
            classify_control_fallback_cluster("\u{0627}\u{200d}", false),
            ControlFallbackCluster::PreserveParleyGlyphs,
        );
    }

    #[test]
    fn dropped_default_ignorable_run_shifts_later_visual_origins() {
        let dropped_runs = [SourceOnlyRun {
            visual_position: ShaperVisualInlinePosition::from_parley(10.0),
            shaper_advance: ShaperInlineAdvance::from_parley(3.0),
            text: Rc::from(""),
        }];

        assert_eq!(
            corrected_visual_inline_position(
                ShaperVisualInlinePosition::from_parley(5.0),
                &dropped_runs,
            )
            .points(),
            5.0
        );
        assert_eq!(
            corrected_visual_inline_position(
                ShaperVisualInlinePosition::from_parley(10.0),
                &dropped_runs,
            )
            .points(),
            10.0
        );
        assert_eq!(
            corrected_visual_inline_position(
                ShaperVisualInlinePosition::from_parley(16.0),
                &dropped_runs,
            )
            .points(),
            13.0
        );
    }

    #[test]
    fn dropped_default_ignorable_correction_is_independent_of_logical_run_order() {
        // RTL runs are emitted in visual order, which can be the reverse of
        // logical text order. The correction therefore depends only on the
        // physical origins Parley provided.
        let dropped_runs = [
            SourceOnlyRun {
                visual_position: ShaperVisualInlinePosition::from_parley(18.0),
                shaper_advance: ShaperInlineAdvance::from_parley(2.0),
                text: Rc::from(""),
            },
            SourceOnlyRun {
                visual_position: ShaperVisualInlinePosition::from_parley(6.0),
                shaper_advance: ShaperInlineAdvance::from_parley(1.5),
                text: Rc::from(""),
            },
        ];

        assert_eq!(
            corrected_visual_inline_position(
                ShaperVisualInlinePosition::from_parley(12.0),
                &dropped_runs,
            )
            .points(),
            10.5
        );
        assert_eq!(
            corrected_visual_inline_position(
                ShaperVisualInlinePosition::from_parley(24.0),
                &dropped_runs,
            )
            .points(),
            20.5
        );
    }

    #[test]
    fn rehomed_control_fragment_stitches_only_following_compatible_visual_runs() {
        let glyph = |unicode: &str| RenderedGlyph {
            kind: RenderedGlyphKind::Paint(1),
            x_advance: 1.0,
            nominal_x_advance: 1.0,
            x_offset: 0.0,
            y_offset: 0.0,
            unicode: unicode.to_string(),
        };
        let run = |text: &str, x_offset: f32| ShapedGlyphRun {
            text: text.into(),
            x_offset,
            y_offset: 0.0,
            text_matrix: RenderedTextMatrix::IDENTITY,
            font_size: 12.0,
            font_id: Some(1),
            font_palette: FontPalette::Normal,
            glyphs: vec![glyph(text)],
            glyph_source_ranges: vec![None],
        };
        let mut runs = vec![run("f\u{200c}", 2.0), run("i", 3.0)];
        assert!(stitch_rehomed_control_fallback_run(&mut runs, 0));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text.as_ref(), "f\u{200c}i");

        let mut reversed_runs = vec![run("f\u{200c}", 3.0), run("i", 2.0)];
        assert!(!stitch_rehomed_control_fallback_run(&mut reversed_runs, 0));
    }
}
