//! Symbolic-resource allocation and late indirect-reference planning.
//!
//! This module is deliberately independent of `Document` and `pdf_writer`.
//! Lowering reserves typed handles; serialization only receives the resolved
//! program produced after this planner has assigned object numbers.

use super::*;

/// A symbolic indirect object produced during PDF lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct PdfSymbolicObject(pub(super) usize);

macro_rules! pdf_resource_handle {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub(super) struct $name(pub(super) PdfSymbolicObject);
    };
}

pdf_resource_handle!(PdfFormHandle);
pdf_resource_handle!(PdfPatternHandle);
pdf_resource_handle!(PdfFunctionHandle);
pdf_resource_handle!(PdfExtGStateHandle);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PdfDynamicResourceKind {
    Form,
    Pattern,
    Function,
    ExtGState,
}

#[derive(Debug, Default, Clone)]
pub(super) struct PdfResourceRegistry {
    resources: Vec<PdfDynamicResourceKind>,
}

impl PdfResourceRegistry {
    fn reserve(&mut self, kind: PdfDynamicResourceKind) -> PdfSymbolicObject {
        let handle = PdfSymbolicObject(self.resources.len());
        self.resources.push(kind);
        handle
    }

    pub(super) fn form(&mut self) -> PdfFormHandle {
        PdfFormHandle(self.reserve(PdfDynamicResourceKind::Form))
    }

    pub(super) fn pattern(&mut self) -> PdfPatternHandle {
        PdfPatternHandle(self.reserve(PdfDynamicResourceKind::Pattern))
    }

    pub(super) fn function(&mut self) -> PdfFunctionHandle {
        PdfFunctionHandle(self.reserve(PdfDynamicResourceKind::Function))
    }

    pub(super) fn ext_gstate(&mut self) -> PdfExtGStateHandle {
        PdfExtGStateHandle(self.reserve(PdfDynamicResourceKind::ExtGState))
    }

    pub(super) fn len(&self) -> usize {
        self.resources.len()
    }
}

/// The sole allocator for private PDF indirect-object numbers. It belongs to
/// the planner rather than serialization so writer helpers cannot reserve
/// objects as a side effect.
#[derive(Debug, Clone, Copy)]
struct PdfObjectAllocator {
    next_id: usize,
}

impl PdfObjectAllocator {
    fn new() -> Self {
        Self { next_id: 1 }
    }

    fn alloc_id(&mut self) -> usize {
        self.reserve_ids(1)
    }

    fn alloc_ids(&mut self, count: usize) -> Vec<usize> {
        let first = self.reserve_ids(count);
        (first..first + count).collect()
    }

    fn reserve_ids(&mut self, count: usize) -> usize {
        let first = self.next_id;
        self.next_id += count;
        first
    }
}

/// Resolved indirect references for the private symbolic resource program.
///
/// ISO 32000-2:2020, 7.3.10 defines indirect object references.
#[derive(Debug, Clone)]
pub(super) struct PdfResourcePlanner {
    allocator: PdfObjectAllocator,
    object_ids: Vec<usize>,
}

impl PdfResourcePlanner {
    pub(super) fn new() -> Self {
        Self {
            allocator: PdfObjectAllocator::new(),
            object_ids: Vec::new(),
        }
    }

    pub(super) fn plan_dynamic_resources(&mut self, registry: &PdfResourceRegistry) {
        let object_ids = self.allocator.alloc_ids(registry.len());
        assert_eq!(registry.len(), object_ids.len());
        assert_eq!(
            object_ids
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            object_ids.len(),
            "every symbolic PDF resource must resolve to one indirect object"
        );
        self.object_ids = object_ids;
    }

    #[cfg(test)]
    pub(super) fn from_object_ids(registry: &PdfResourceRegistry, object_ids: Vec<usize>) -> Self {
        assert_eq!(registry.len(), object_ids.len());
        Self {
            allocator: PdfObjectAllocator {
                next_id: object_ids.iter().copied().max().unwrap_or(0) + 1,
            },
            object_ids,
        }
    }

    pub(super) fn alloc_id(&mut self) -> usize {
        self.allocator.alloc_id()
    }
    pub(super) fn alloc_ids(&mut self, count: usize) -> Vec<usize> {
        self.allocator.alloc_ids(count)
    }
    pub(super) fn reserve_ids(&mut self, count: usize) -> usize {
        self.allocator.reserve_ids(count)
    }
    pub(super) fn peek_id(&self) -> usize {
        self.allocator.next_id
    }
    pub(super) fn advance_to(&mut self, next_id: usize) {
        assert!(
            next_id >= self.allocator.next_id,
            "PDF object allocator cannot move backwards"
        );
        self.allocator.next_id = next_id;
    }

    fn resolve(&self, object: PdfSymbolicObject) -> usize {
        self.object_ids[object.0]
    }
    pub(super) fn form(&self, handle: PdfFormHandle) -> usize {
        self.resolve(handle.0)
    }
    pub(super) fn pattern(&self, handle: PdfPatternHandle) -> usize {
        self.resolve(handle.0)
    }
    pub(super) fn function(&self, handle: PdfFunctionHandle) -> usize {
        self.resolve(handle.0)
    }
    pub(super) fn ext_gstate(&self, handle: PdfExtGStateHandle) -> usize {
        self.resolve(handle.0)
    }
    pub(super) fn object_ids(&self) -> impl Iterator<Item = usize> + '_ {
        self.object_ids.iter().copied()
    }

    /// Resolve typed local stream bindings after the complete object schedule
    /// is known. This deliberately accepts no resource-name lookup tables.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve_stream_bindings(
        &self,
        stream: &mut PdfStreamProgram,
        fonts: &[EmbeddedFontPlan<'_>],
        image_ids: &[Option<ImageObjectIds>],
        image_patterns: &[Vec<PageImagePatternPlan>],
        page_ext_gstates: &[Vec<ExtGStateObjectPlan>],
        color_plan: &crate::pdf::colors::PdfColorPlan,
    ) {
        let mut resolved = PdfResolvedStreamResources::default();
        for (name, handle) in &stream.resource_uses.fonts {
            resolved
                .fonts
                .insert(name.clone(), PdfResolvedReference(fonts[handle.0].type0_id));
        }
        for (name, handle) in &stream.resource_uses.xobjects {
            let id = match handle {
                PdfXObjectHandle::Form(handle) => self.form(*handle),
                PdfXObjectHandle::Image(handle) => {
                    image_ids[handle.0]
                        .expect("typed raster image binding has no image object")
                        .image_id
                        .0
                }
            };
            resolved
                .xobjects
                .insert(name.clone(), PdfResolvedReference(id));
        }
        for (name, handle) in &stream.resource_uses.patterns {
            let id = match handle {
                PdfPatternResourceHandle::Dynamic(handle) => self.pattern(*handle),
                PdfPatternResourceHandle::Image(handle) => {
                    image_patterns[handle.page_index]
                        .iter()
                        .find(|plan| plan.handle == *handle)
                        .expect("typed image-pattern binding has no pattern object")
                        .id
                }
            };
            resolved
                .patterns
                .insert(name.clone(), PdfResolvedReference(id));
        }
        for (name, handle) in &stream.resource_uses.ext_gstates {
            let id = match handle {
                PdfExtGStateResourceHandle::Dynamic(handle) => self.ext_gstate(*handle),
                PdfExtGStateResourceHandle::Page(handle) => {
                    page_ext_gstates[handle.page_index][handle.resource_index].id
                }
            };
            resolved
                .ext_gstates
                .insert(name.clone(), PdfResolvedReference(id));
        }
        for (name, handle) in &stream.resource_uses.color_spaces {
            resolved.color_spaces.insert(
                name.clone(),
                PdfResolvedReference(color_plan.resource_object_for_handle(handle)),
            );
        }
        stream.resolved_resources = Some(resolved);
    }
}
