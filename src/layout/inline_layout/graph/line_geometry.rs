use super::*;

pub(super) fn selected_line_tab_advance_adjustment<T>(
    items: &[T],
    font_system: &mut FontSystem,
    tab_metric_style: &ComputedStyle,
    item_width: impl Fn(&T) -> f32,
) -> f32
where
    T: AsRef<InlineLineItem>,
{
    let mut cursor = 0.0;
    let mut adjustment = 0.0;
    let mut index = 0;
    while index < items.len() {
        let InlineLineItem::Fragment(first_fragment) = items[index].as_ref() else {
            cursor += item_width(&items[index]);
            index += 1;
            continue;
        };

        let start = index;
        let mut spans = Vec::new();
        let mut text = String::new();
        let mut unadjusted_width = 0.0;
        let mut has_tab = false;
        while let Some(item) = items.get(index) {
            let InlineLineItem::Fragment(fragment) = item.as_ref() else {
                break;
            };
            has_tab |= fragment.text().contains('\t');
            spans.push(StyledTextSpan {
                text: fragment.text(),
                style: fragment.style(),
            });
            text.push_str(fragment.text());
            unadjusted_width += item_width(item);
            index += 1;
        }
        debug_assert!(index > start);
        let used_width = if has_tab {
            font_system
                .shape_untracked_styled_inline_fragments(
                    &spans,
                    text,
                    0.0,
                    first_fragment.style().line_height,
                    cursor,
                    tab_metric_style,
                )
                .map(|shaped| shaped.advance_width())
                .unwrap_or(unadjusted_width)
        } else {
            unadjusted_width
        };
        adjustment += used_width - unadjusted_width;
        cursor += used_width;
    }
    adjustment
}

/// Tabs depend on the used inline cursor, while ruby overhang can reduce the
/// preceding atom's used advance. Iterate the two selected-line operations to
/// a small fixed point so a tab after ruby never retains a stop calculated
/// from the source atom's conservative annotation width.
pub(super) fn resolve_materialized_line_tab_and_ruby_geometry(
    items: &mut [MeasuredInlineItem],
    font_system: &mut FontSystem,
    block_style: &ComputedStyle,
) {
    if !items.iter().any(|item| {
        matches!(
            &item.item,
            InlineLineItem::Fragment(fragment) if fragment.text().contains('\t')
        ) || matches!(
            &item.item,
            InlineLineItem::Atom(atom) if matches!(atom.content(), InlineAtomContent::Ruby { .. })
        )
    }) {
        return;
    }
    const MAX_GEOMETRY_PASSES: usize = 4;
    for _ in 0..MAX_GEOMETRY_PASSES {
        let tabs_changed = resolve_materialized_line_tab_advances(items, font_system, block_style);
        let ruby_changed = resolve_materialized_ruby_overhang(items, font_system, block_style);
        if !tabs_changed && !ruby_changed {
            break;
        }
    }
}

/// Whether a used inline advance changed enough to require another geometry
/// pass. `NaN` remains non-convergent, matching the former direct comparison.
fn materialized_inline_geometry_changed(previous: f32, current: f32) -> bool {
    match (previous - current).abs().partial_cmp(&0.01) {
        Some(std::cmp::Ordering::Less) => false,
        Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater) | None => true,
    }
}

/// Resolve ruby annotation overlap against the selected parent inline line.
///
/// Ruby source atoms are conservatively sized to their widest annotation for
/// graph fitting. Once CSS Text has selected, trimmed, and tab-resolved a
/// line, the annotation can borrow the permitted adjacent inline space and
/// expose its smaller normal-flow base-column span. This pass owns no source
/// text and clones only the selected atom, so another candidate line cannot
/// inherit a placement from this one.
/// <https://drafts.csswg.org/css-ruby-1/#ruby-overhang>
fn resolve_materialized_ruby_overhang(
    items: &mut [MeasuredInlineItem],
    font_system: &mut FontSystem,
    block_style: &ComputedStyle,
) -> bool {
    let mut geometry_changed = false;
    for item_index in 0..items.len() {
        let Some(placement) =
            resolved_ruby_placement_for_line_item(items, item_index, font_system, block_style)
        else {
            continue;
        };
        let flow_span = placement.flow_inline_span.points();
        let InlineLineItem::Atom(atom) = &items[item_index].item else {
            continue;
        };
        let mut atom = atom.clone().with_ruby_placement(placement);
        match block_style.writing_mode {
            WritingMode::HorizontalTb => atom.size.width = flow_span,
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => atom.size.height = flow_span,
        }
        geometry_changed |= materialized_inline_geometry_changed(
            items[item_index].base_advance().points(),
            flow_span,
        );
        items[item_index].advance.replace_base_points(flow_span);
        items[item_index].item = InlineLineItem::Atom(atom);
    }
    geometry_changed
}

fn resolved_ruby_placement_for_line_item(
    items: &[MeasuredInlineItem],
    item_index: usize,
    font_system: &mut FontSystem,
    block_style: &ComputedStyle,
) -> Option<ruby::ResolvedRubyPlacement> {
    let InlineLineItem::Atom(atom) = &items.get(item_index)?.item else {
        return None;
    };
    let InlineAtomContent::Ruby {
        base, annotations, ..
    } = atom.content()
    else {
        return None;
    };
    let column_span = base.containing_inline_size.points();
    let spaces = ruby_adjacent_space_allowance(items, item_index, block_style);
    let mut levels = Vec::with_capacity(annotations.len());
    let mut maximum_start = 0.0_f32;
    let mut maximum_end = 0.0_f32;
    for annotation in annotations {
        let available_span = annotation.containing_inline_size.points();
        let paint_span = annotation.paint_inline_size.points();
        let (alignment_offset, overhang) =
            ruby_alignment_geometry(annotation.style.ruby_align, available_span, paint_span);
        let policy_allowance = match annotation.overhang_policy {
            css::RubyOverhang::Spaces => spaces,
            css::RubyOverhang::Auto => ruby_auto_overhang_allowance(
                items,
                item_index,
                annotation.style.as_ref(),
                font_system,
            ),
        };
        let resolved_overhang = resolve_ruby_overhang(overhang, policy_allowance);
        maximum_start = maximum_start.max(resolved_overhang.unborrowed.inline_start.points());
        maximum_end = maximum_end.max(resolved_overhang.unborrowed.inline_end.points());
        levels.push((alignment_offset, overhang, resolved_overhang));
    }
    Some(ruby::ResolvedRubyPlacement {
        flow_inline_span: ruby::RubyColumnInlineSpan::new(
            column_span + maximum_start + maximum_end,
        ),
        base_inline_offset: ruby::RubyInlineDisplacement::new(maximum_start),
        annotation_inline_offsets: levels
            .iter()
            .map(|(alignment_offset, overhang, _)| {
                debug_assert!(
                    (*alignment_offset + overhang.inline_start.points()).abs() < 0.01
                        || overhang.inline_start.points() == 0.0
                );
                // `alignment_offset` includes the negative start overhang;
                // the common flow start retains only unborrowed excess.
                ruby::RubyInlineDisplacement::new(maximum_start + *alignment_offset)
            })
            .collect(),
        overhang: levels
            .into_iter()
            .map(|(_, _, resolved)| resolved)
            .collect(),
    })
}

/// Consume the selected line's independent start and end offers. Any excess
/// that cannot be borrowed stays in the ruby atom's normal-flow span.
pub(super) fn resolve_ruby_overhang(
    overhang: ruby::RubyAlignedOverhang,
    allowance: ruby::RubyOverhangAllowance,
) -> ruby::ResolvedRubyOverhang {
    let borrowed_start = overhang
        .inline_start
        .points()
        .min(allowance.inline_start.points());
    let borrowed_end = overhang
        .inline_end
        .points()
        .min(allowance.inline_end.points());
    ruby::ResolvedRubyOverhang {
        borrowed: ruby::RubyOverhangAllowance {
            inline_start: ruby::RubyInlineSpan::new(borrowed_start),
            inline_end: ruby::RubyInlineSpan::new(borrowed_end),
        },
        unborrowed: ruby::RubyAlignedOverhang {
            inline_start: ruby::RubyInlineSpan::new(
                (overhang.inline_start.points() - borrowed_start).max(0.0),
            ),
            inline_end: ruby::RubyInlineSpan::new(
                (overhang.inline_end.points() - borrowed_end).max(0.0),
            ),
        },
    }
}

pub(super) fn ruby_alignment_geometry(
    align: css::RubyAlign,
    available_span: f32,
    paint_span: f32,
) -> (f32, ruby::RubyAlignedOverhang) {
    if paint_span <= available_span {
        let offset = match align {
            css::RubyAlign::Start => 0.0,
            css::RubyAlign::Center | css::RubyAlign::SpaceBetween | css::RubyAlign::SpaceAround => {
                (available_span - paint_span) / 2.0
            }
        };
        return (offset, ruby::RubyAlignedOverhang::default());
    }
    let excess = paint_span - available_span;
    let start = match align {
        css::RubyAlign::Start => 0.0,
        css::RubyAlign::Center | css::RubyAlign::SpaceBetween | css::RubyAlign::SpaceAround => {
            excess / 2.0
        }
    };
    (
        -start,
        ruby::RubyAlignedOverhang {
            inline_start: ruby::RubyInlineSpan::new(start),
            inline_end: ruby::RubyInlineSpan::new(excess - start),
        },
    )
}

pub(super) fn ruby_adjacent_space_allowance(
    items: &[MeasuredInlineItem],
    item_index: usize,
    block_style: &ComputedStyle,
) -> ruby::RubyOverhangAllowance {
    ruby::RubyOverhangAllowance {
        inline_start: ruby::RubyInlineSpan::new(
            item_index
                .checked_sub(1)
                .and_then(|index| ruby_inline_end_space_offer(&items[index], block_style))
                .unwrap_or(0.0),
        ),
        inline_end: ruby::RubyInlineSpan::new(
            items
                .get(item_index + 1)
                .and_then(|item| ruby_inline_start_space_offer(item, block_style))
                .unwrap_or(0.0),
        ),
    }
}

fn ruby_auto_overhang_allowance(
    items: &[MeasuredInlineItem],
    item_index: usize,
    style: &ComputedStyle,
    font_system: &mut FontSystem,
) -> ruby::RubyOverhangAllowance {
    let maximum = font_system.ic_advance_for_style(style).points() / 2.0;
    let neighbor_width = |index: Option<usize>| {
        index
            .and_then(|index| items.get(index))
            .filter(|item| !matches!(item.item, InlineLineItem::Atom(_)))
            .map_or(0.0, |item| {
                ruby_auto_overhang_offer(item.base_advance().points(), maximum)
            })
    };
    ruby::RubyOverhangAllowance {
        inline_start: ruby::RubyInlineSpan::new(neighbor_width(item_index.checked_sub(1))),
        inline_end: ruby::RubyInlineSpan::new(neighbor_width(
            (item_index + 1 < items.len()).then_some(item_index + 1),
        )),
    }
}

/// Quire's deterministic `auto` policy: never borrow more than half an `ic`
/// from either immediate visual neighbor.
pub(super) fn ruby_auto_overhang_offer(neighbor_inline_span: f32, half_ic: f32) -> f32 {
    neighbor_inline_span.max(0.0).min(half_ic.max(0.0))
}

fn ruby_inline_end_space_offer(
    item: &MeasuredInlineItem,
    block_style: &ComputedStyle,
) -> Option<f32> {
    ruby_inline_adjacent_space_offer(item, block_style, true)
}

fn ruby_inline_start_space_offer(
    item: &MeasuredInlineItem,
    block_style: &ComputedStyle,
) -> Option<f32> {
    ruby_inline_adjacent_space_offer(item, block_style, false)
}

fn ruby_inline_adjacent_space_offer(
    item: &MeasuredInlineItem,
    block_style: &ComputedStyle,
    at_end: bool,
) -> Option<f32> {
    let InlineLineItem::Fragment(fragment) = &item.item else {
        return None;
    };
    let text = fragment.text();
    let vertical = matches!(
        block_style.text_layout_policy(),
        css::TextLayoutPolicy::Vertical(_)
    );
    let boundary = if at_end {
        text.char_indices().next_back()
    } else {
        text.char_indices().next()
    }?;
    let (offset, character) = boundary;
    let character_end = offset + character.len_utf8();
    let punctuation = crate::text::text_spacing_punctuation_class(
        character,
        fragment.style().language.as_deref(),
        vertical,
    );
    let punctuation_share = ruby_punctuation_overhang_share(
        at_end,
        punctuation,
        fragment.style().text_spacing_trim.resolved(),
    );
    if let Some(share) = punctuation_share {
        return ruby_fragment_source_range_width(item, offset..character_end)
            .map(|width| width * share);
    }
    let is_eligible_space =
        |character: char| ruby_overhang_space_is_eligible(character, fragment.style());
    if !is_eligible_space(character) {
        return None;
    }
    let range = if at_end {
        let start = text
            .char_indices()
            .rev()
            .take_while(|(_, character)| is_eligible_space(*character))
            .last()
            .map_or(text.len(), |(offset, _)| offset);
        start..text.len()
    } else {
        let end = text
            .char_indices()
            .take_while(|(_, character)| is_eligible_space(*character))
            .last()
            .map_or(0, |(offset, character)| offset + character.len_utf8());
        0..end
    };
    ruby_fragment_source_range_width(item, range)
}

pub(super) fn ruby_punctuation_overhang_share(
    at_end: bool,
    punctuation: Option<crate::text::TextSpacingPunctuationClass>,
    text_spacing_trim: TextSpacingTrim,
) -> Option<f32> {
    if text_spacing_trim != TextSpacingTrim::SpaceAll {
        return None;
    }
    match (at_end, punctuation) {
        (true, Some(crate::text::TextSpacingPunctuationClass::Closing))
        | (false, Some(crate::text::TextSpacingPunctuationClass::Opening)) => Some(0.5),
        (_, Some(crate::text::TextSpacingPunctuationClass::MiddleDot)) => Some(0.25),
        _ => None,
    }
}

/// The `spaces` policy considers preserved document spaces/tabs, U+00A0, and
/// Unicode General_Category `Zs` characters. It does not treat arbitrary
/// control whitespace as borrowable inline space.
pub(super) fn ruby_overhang_space_is_eligible(character: char, style: &ComputedStyle) -> bool {
    (matches!(character, ' ' | '\t') && !style.white_space.collapses_spaces())
        || matches!(character, '\u{00a0}')
        || crate::text::character_is_css_other_space_separator(character)
}

fn ruby_fragment_source_range_width(
    item: &MeasuredInlineItem,
    range: std::ops::Range<usize>,
) -> Option<f32> {
    let InlineLineItem::Fragment(fragment) = &item.item else {
        return None;
    };
    let shaped = item.shaped.as_deref()?;
    if range.start == 0 && range.end == fragment.text().len() {
        return Some(item.base_advance().points());
    }
    let range_width = shaped.source_range_advance_width(range.clone())?;
    // Tab expansion has already updated the complete fragment's used width.
    // Derive a leading/trailing tab run from the complementary shaped range so
    // it receives the actual selected tab-stop advance.
    if range.start == 0 {
        let remainder = shaped
            .source_range_advance_width(range.end..fragment.text().len())
            .unwrap_or(0.0);
        Some((item.base_advance().points() - remainder).max(0.0))
    } else if range.end == fragment.text().len() {
        let prefix = shaped
            .source_range_advance_width(0..range.start)
            .unwrap_or(0.0);
        Some((item.base_advance().points() - prefix).max(0.0))
    } else {
        Some(range_width)
    }
}

/// Resolve preserved tabs into the selected line's fragment measurements.
///
/// A tab stop depends on the preceding *used* inline cursor, so keeping its
/// advance only as an aggregate line-width correction leaves the tab's own
/// background, decoration, and initial-letter pseudo geometry at zero width.
/// Re-slice the complete selected text group instead: every fragment then
/// owns the same advance that line fitting and paint use.
/// <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
pub(super) fn resolve_materialized_line_tab_advances(
    items: &mut [MeasuredInlineItem],
    font_system: &mut FontSystem,
    tab_metric_style: &ComputedStyle,
) -> bool {
    let mut cursor = 0.0;
    let mut index = 0;
    let mut geometry_changed = false;
    while index < items.len() {
        let InlineLineItem::Fragment(_) = &items[index].item else {
            cursor += items[index].used_advance().points();
            index += 1;
            continue;
        };
        let start = index;
        let mut has_tab = false;
        while let Some(item) = items.get(index) {
            let InlineLineItem::Fragment(fragment) = &item.item else {
                break;
            };
            has_tab |= fragment.text().contains('\t');
            index += 1;
        }
        debug_assert!(index > start);
        if !has_tab {
            cursor += items[start..index]
                .iter()
                .map(|item| item.used_advance().points())
                .sum::<f32>();
            continue;
        }
        // Tracking is represented as a boundary advance on the following
        // typographic unit. Feed that advance into the cursor *before*
        // resolving the fragment's tab, rather than reshaping the complete
        // untracked group and appending all spacing afterwards. This keeps
        // tab selection, graph fitting, and paint on the same used cursor.
        // <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
        for item in &mut items[start..index] {
            cursor += item.advance.boundary_before().points();
            let InlineLineItem::Fragment(fragment) = &item.item else {
                unreachable!("a contiguous text group only contains fragments");
            };
            let width = if fragment.text().contains('\t') {
                let span = [StyledTextSpan {
                    text: fragment.text(),
                    style: fragment.style(),
                }];
                if let Some(shaped) = font_system.shape_untracked_styled_inline_fragments(
                    &span,
                    fragment.text().to_owned(),
                    0.0,
                    fragment.style().line_height,
                    cursor,
                    tab_metric_style,
                ) {
                    let width = shaped.advance_width();
                    geometry_changed |=
                        materialized_inline_geometry_changed(item.base_advance().points(), width);
                    item.advance.replace_base_points(width);
                    item.shaped = Some(Rc::new(shaped));
                    width
                } else {
                    item.base_advance().points()
                }
            } else {
                item.base_advance().points()
            };
            cursor += width;
        }
    }
    geometry_changed
}
