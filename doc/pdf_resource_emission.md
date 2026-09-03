# PDF Resource Emission

Spindrift emits regular-PDF resources only when the rendered document can refer to
them. An unpainted ordinary-PDF page therefore has no content stream,
calibrated color profile, or XMP packet when the source supplies no document
metadata. A page still carries its required empty `/Resources` dictionary; the
PDF information dictionary identifies the producer.

Painted ordinary PDFs retain their calibrated ICC color spaces, fonts, image
resources, and source metadata. PDF/A output takes precedence over compactness:
it always emits XMP identification metadata and its sRGB output intent, even
for an unpainted page.

This follows PDF resource dictionaries' role as name scopes for content
operators (ISO 32000-2:2020, 7.8.3) and avoids declaring names that no content
stream can use. It does not change Spindrift's default PDF 1.4 compatibility
target.

## Symbolic resource planning

PDF lowering produces a private symbolic document program before assigning
indirect-object numbers. A final operator audit records each content stream's
Font, XObject, Pattern, ExtGState, and ColorSpace names; the later resource
planner resolves typed Form, Pattern, Function, and ExtGState handles in
deterministic encounter order. In particular, a Form only receives its own
direct nested Form dependencies, rather than a copy of the page's Form table.
This follows the PDF 2.0 requirement for independent Form XObjects to supply
their named resources and is emitted for Spindrift's PDF 1.4 output as an
interoperability discipline.

Every emitted content stream is frozen at that boundary, including raster and
SVG tiling-pattern tiles plus gradient tiles and alpha-mask Forms. Its
resolved resource dictionary is written directly from the completed bindings;
the serializer neither scans global resource tables nor regenerates operators.
The allocation baseline intentionally has no legacy empty font-resource slot.

Each frozen name is paired with a typed symbolic target (font, image, Form,
pattern, ExtGState, or colour space) before indirect objects exist. The final
operator audit validates that those local bindings select the corresponding
operators; planning resolves the bindings once without a document-wide
name-to-resource lookup. The planner owns the indirect-object allocator, and
the serializer accepts only the resolved program plus its PDF profile and
compression settings.

The resolved document program also owns its page/content entries, embedded
fonts, deduplicated images and image patterns, ExtGStates, lowered link
annotations, metadata objects, and outline plan. Serializers consume that
program rather than re-deriving static entries from source page state; a
program invariant asserts that all resolved indirect references are unique.

An isolated transparency Form explicitly selects its sRGB ICCBased blending
space. That requirement is independent from CSS paint colour spaces, so a
colourless isolated Form still causes the required sRGB profile to be planned.
Spindrift first lowers against a provisional policy with no indirect references,
then derives the resolved ICC plan from the final PDF paint operators,
normalized gradients, SVG tile streams, and emitted Form kinds. This preserves
CSS conversion and interpolation semantics while allowing unused source paint
records to be pruned. Raster ICC profiles follow the same rule: an image
profile is retained only when a final image `Do` or image-pattern paint can
reach it; direct solid-image fills are represented by their final color-space
operators instead.
PDF/A's document OutputIntent remains a separate, always-required conformance
resource; an otherwise blank ordinary PDF retains no output intent.

## Module boundary

The private backend is organized around this boundary: `pdf/planner.rs` owns
symbolic handles, the deterministic allocation schedule, and binding
resolution; `pdf/serialize/` owns shared stream encoding plus page, resource,
font, and raster-image serialization. The remaining `pdf/writer.rs` is the
lowering and program-assembly orchestration point, while its graphics helpers
are kept together because PDF gradient functions, alpha masks, Forms, and
ExtGStates share one dependency graph. No serializer accepts a `Document`.
