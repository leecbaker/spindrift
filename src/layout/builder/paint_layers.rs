use super::*;
use crate::layout::assets::DocumentPageIndex;

pub(in crate::layout) fn append_fixed_layer_to_page(page: &mut Page, layer: &FixedPaintLayer) {
    let fragment = fixed_layer_fragment(layer);
    let recorded = page.record_paint_fragment_owned(fragment, PaintTranslation::identity());
    page.append_recorded_paint_fragment(recorded);
    page.sort_paint_tree_stacking_contexts();
}

pub(in crate::layout) fn positioned_layer_fragment(layer: &PositionedPaintLayer) -> PaintFragment {
    PaintFragment::from_stacking_context(layer.context.clone().with_links(layer.links.clone()))
}

pub(in crate::layout) fn fixed_layer_fragment(layer: &FixedPaintLayer) -> PaintFragment {
    PaintFragment::from_stacking_context(layer.context.clone().with_links(layer.links.clone()))
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn next_paint_source_order(&mut self) -> usize {
        let source_order = self.next_paint_source_order;
        self.next_paint_source_order += 1;
        source_order
    }

    pub(in crate::layout) fn append_or_defer_scoped_paint_fragment(
        &mut self,
        page_index: usize,
        fragment: PaintFragment,
    ) {
        if page_index < self.pages.len() {
            self.pages[page_index]
                .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            self.pages[page_index].sort_paint_tree_stacking_contexts();
        } else if page_index == self.pages.len() {
            self.current_page
                .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            self.current_page.sort_paint_tree_stacking_contexts();
        } else {
            self.pending_paint_fragments.push(PendingPaintFragment {
                page_index,
                fragment,
                kind: PendingPaintFragmentKind::PositionedOrScoped,
            });
        }
    }

    pub(in crate::layout) fn flush_positioned_layers(&mut self) {
        if self.positioned_layers.is_empty() || self.positioned_paint_transaction_depth > 0 {
            return;
        }
        let mut future_layers = Vec::new();
        let mut positioned_layers = Vec::new();
        for layer in std::mem::take(&mut self.positioned_layers) {
            if layer.page_index > self.pages.len() {
                future_layers.push(layer);
            } else {
                positioned_layers.push(layer);
            }
        }
        self.positioned_layers = future_layers;
        if positioned_layers.is_empty() {
            return;
        }
        positioned_layers.sort_by_key(|layer| {
            (
                layer.page_index,
                layer.stack_level.sort_key(),
                layer.context.source_order,
            )
        });
        for layer in positioned_layers {
            if let Some(identity) = layer.commit_key() {
                // A positioned principal can be reached through an inline
                // collector's retained source and its ruby-specific overlay.
                // They describe the same page-local principal, which must be
                // committed once even when both collection paths survive to
                // final paint.
                if !self
                    .committed_positioned_paint_identities
                    .insert((DocumentPageIndex::new(layer.page_index), identity))
                {
                    continue;
                }
            }
            let fragment = positioned_layer_fragment(&layer);
            let target_page = if layer.page_index < self.pages.len() {
                &mut self.pages[layer.page_index]
            } else {
                &mut self.current_page
            };
            let recorded =
                target_page.record_paint_fragment_owned(fragment, PaintTranslation::identity());
            target_page.append_recorded_paint_fragment(recorded);
            target_page.sort_paint_tree_stacking_contexts();
        }
    }
    pub(in crate::layout) fn flush_positioned_layers_since(&mut self, start_index: usize) {
        if start_index >= self.positioned_layers.len() {
            return;
        }
        let mut subtree_layers = self.positioned_layers.split_off(start_index);
        subtree_layers.sort_by_key(|layer| layer.stack_level.sort_key());
        for layer in subtree_layers {
            let fragment = positioned_layer_fragment(&layer);
            self.current_page
                .append_paint_fragment_owned(fragment, PaintTranslation::identity());
        }
    }

    pub(in crate::layout) fn apply_fixed_layers_to_pages(&mut self) {
        if self.fixed_layers.is_empty() {
            return;
        }
        self.fixed_layers
            .sort_by_key(|layer| (layer.stack_level.sort_key(), layer.context.source_order));
        let fixed_layers = self.fixed_layers.clone();
        for page in &mut self.pages {
            for layer in &fixed_layers {
                append_fixed_layer_to_page(page, layer);
            }
        }
    }
}
