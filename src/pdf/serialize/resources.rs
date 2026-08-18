//! Direct resource-dictionary serialization for resolved content streams.

use super::super::*;
use super::primitives::{pdf_name, pdf_ref};

/// Emit only the resource bindings declared by one resolved stream program.
///
/// PDF resource dictionaries are scoped to their content stream or Form; the
/// planner has already resolved each local symbolic binding to an indirect
/// reference by this point.
pub(crate) fn write_resource_dictionary(
    resources: &mut pdf_writer::writers::Resources<'_>,
    bindings: &PdfResolvedStreamResources,
) {
    if !bindings.fonts.is_empty() {
        let mut fonts = resources.fonts();
        for (name, reference) in &bindings.fonts {
            fonts.pair(pdf_name(name), pdf_ref(reference.0));
        }
    }
    if !bindings.color_spaces.is_empty() {
        let mut color_spaces = resources.color_spaces();
        for (name, reference) in &bindings.color_spaces {
            color_spaces
                .insert(pdf_name(name))
                .start::<pdf_writer::writers::ColorSpace>()
                .icc_based(pdf_ref(reference.0));
        }
    }
    if !bindings.xobjects.is_empty() {
        let mut xobjects = resources.x_objects();
        for (name, reference) in &bindings.xobjects {
            xobjects.pair(pdf_name(name), pdf_ref(reference.0));
        }
    }
    if !bindings.patterns.is_empty() {
        let mut patterns = resources.patterns();
        for (name, reference) in &bindings.patterns {
            patterns.pair(pdf_name(name), pdf_ref(reference.0));
        }
    }
    if !bindings.ext_gstates.is_empty() {
        let mut ext_gstates = resources.ext_g_states();
        for (name, reference) in &bindings.ext_gstates {
            ext_gstates.pair(pdf_name(name), pdf_ref(reference.0));
        }
    }
}
