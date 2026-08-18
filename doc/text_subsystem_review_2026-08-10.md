# Text subsystem review

Reviewed `src/text.rs` and all production modules beneath `src/text/` on 2026-08-10. The follow-up entries below record implementation work completed from this review.

## Executive summary

The subsystem has a strong overall direction. It keeps CSS line-breaking, UAX #9 reordering, OpenType shaping, and PDF emission distinct, and its use of durable shaped-line artifacts is the right foundation for fidelity. The main design problem is that several shaping-only text transformations are represented as plain `String`s without a first-class mapping back to the authored UTF-8 stream. That creates two correctness defects today and causes a large share of the avoidable copying in the styled shaping path.

Recommended order:

1. Fix source-coordinate preservation for injected emoji selectors.
2. Introduce one mapped shaping-input representation and route every insertion/removal through it.
3. Remove the immediate per-run/per-glyph allocations listed below.
4. Split the very large styled-shaping conversion function around that representation.

## Findings

### P1 — injected emoji selectors invalidate shaped source ranges

`text_with_font_variant_emoji` can insert U+FE0E/U+FE0F before the text is handed to Parley (`src/text/system/api/shaping/lines.rs:78-89`). The inserted selector changes UTF-8 byte offsets, but `rendered_text_runs_for_parley_line` writes Parley's internal cluster ranges directly to `glyph_source_ranges` (`src/text/system/api/shaping/styled.rs:248-335`). `shape_unwrapped_line` then stores the original, selector-free input in `ShapedInlineLine::text` (`lines.rs:191-204`).

Consequently, `ShapedInlineLine::source_slice` and `source_range_advance_width` compare ranges from the augmented buffer against a different string (`src/text.rs:342-505`). A selector before the requested slice makes later ranges shifted; a selector inside the slice makes a glyph range larger than the authored cluster. The conservative checks normally return `None`, but that forces the caller to reshape a selected fragment. For joining scripts, that loses the contextual glyph forms this cache exists to preserve.

The styled route has the same issue more directly: `styled_text_source_positions` only knows about synthetic join ranges (`styled.rs:1475-1515`), not emoji selectors injected by `push_text_with_font_variant_emoji` (`styled.rs:810`). When it meets an inserted selector it fails the character-by-character correspondence and returns an empty position map.

Fix this before performance work. Model inserted selectors as zero-source-width spans in a source map, map every Parley cluster through it, and retain authored ranges only. Add regression tests that source-slice text before and after an emoji-presentation base, including a following Arabic joining sequence.

### Resolved P1 — bidi visual-shaping guards now remap source coordinates

`shape_visual_ordered_line` and `shape_visually_ordered_inline_fragments` wrap their input in LRO/PDF to prevent a second UAX #9 resolution pass. On 2026-08-10, both paths were changed to translate each backend cluster range from that guarded input to the retained, unguarded `ShapedInlineLine::text`. A cluster that also contains a guard is intersected with the authored middle segment; guard-only non-painting artifacts are discarded.

This restores the documented invariant that every retained `ShapedInlineGlyph::source_range` indexes `ShapedInlineLine::text`. Regression tests cover both plain and styled visual shaping, verify byte and scalar boundaries, and confirm that a complete source selection remains reusable without re-shaping.

The broader mapped-shaping-input representation is still warranted for emoji selectors and other synthetic text transformations.

### P2 — COLR conversion can paint an incorrect image for clipped, transformed, or composited glyphs

`ColrPathPainter` intentionally ignores `push_clip`, `push_clip_box`, `push_transform`, `pop_transform`, `push_layer`, and `pop_layer` (`src/text/system/api/glyphs/color.rs:325-331`). `paint_color_glyph` can therefore report success and remove the original glyph from the PDF-font path (`color.rs:80-91`) even though the returned vector paths omit required COLR behavior. The custom COLR v0 override path has the same limitation for any feature beyond simple layers.

Do not claim successful path conversion unless every callback required by the glyph is represented. A safe interim policy is to retain the glyph for the alternative paint path when the painter sees unsupported operations. The complete solution needs a typed COLR paint graph/stack that can carry clip, transform, and compositing state into Quire paint operations. This is a fidelity issue, not merely a missing enhancement.

### Resolved P2 — raster PNG validation rejects mismatched dimensions

On 2026-08-10, `decode_png_raster_glyph` was changed to validate the decoded
PNG width and height independently against `RasterGlyphImage::{width,height}`.
The decoder now rejects a transposed or otherwise mismatched image even when
its pixel count is identical to the strike record, preventing it from being
positioned and scaled at the wrong aspect ratio. A regression test encodes a
2×1 PNG with a 1×2 strike record and asserts that conversion fails.

### Resolved P2 — WOFF reconstruction validates total size and directory range

On 2026-08-10, the WOFF decoder was changed to calculate the fully aligned sfnt
layout before decompressing any table, then require it to equal the header's
`totalSfntSize`. It rejects offset/length values outside the `u32` sfnt
directory range and caps reconstructed sfnt programs at 256 MiB before
decompression or allocation. This prevents a malformed container from
truncating directory offsets, overstating its reconstructed size, or forcing
an impractically large decompression allocation. Regression tests cover both a
mismatched declared size and a valid matching reconstruction.

### Partially resolved P2 — styled shaping input now has explicit ownership and provenance boundaries

`shape_styled_text_runs_with_parley_at_tab_origin_with_letter_spacing` is about 650 lines and successively builds:

- `unicode_range_spans` with owned text and optional owned style (`styled.rs:702-724`);
- `metric_styles`, then a second `StyledTextSpan` vector, then another `(span, metric)` vector (`725-746`);
- `authored_text`, `shaping_spans`, augmented `text`, `ranges`, `metric_ranges`, synthetic ranges, source positions, Parley styles, feature contexts, and family-source strings.

The function does important work, but the former representation made ownership
and coordinate-system invariants hard to audit. It is also why new
transformations have missed source-map updates.

On 2026-08-10, the first two boundaries were extracted:

1. `resolved_styled_text_spans` resolves `unicode-range` face selection while
   retaining the authored metric style.
2. `MappedStyledShapingText` owns the augmented Parley input and carries the
   source-position mapping used to translate its non-synthetic clusters to
   authored CSS Text.

The caller retains the resolved span storage, and the mapped ranges borrow it.
This makes the owned selected-style lifetime explicit without cloning
unchanged computed styles or creating self-referential state.

The remaining high-value extraction is to make the two later stages explicit:

1. `ParleyStylePlan<'a>`: contiguous internal ranges with already-resolved
   font family, shaping settings, metrics, and tab context.
2. `convert_parley_line`: the sole place that converts internal cluster
   ranges into authored provenance and `ShapedGlyphRun`s.

This keeps CSS style selection, source transformation, Parley configuration,
and glyph conversion independently testable. It also supplies the architectural
boundary needed for the remaining emoji-selector provenance fix without
weakening boundary shaping.

## Completed allocation follow-ups

- **2026-08-10 — styled shaping span ownership.** Normal styled spans now
  borrow their text and `ComputedStyle`; unicode-range selection allocates a
  replacement style only for a contiguous changed-face span. The intermediate
  span/style vectors were removed. The durable resolved-span vector reserves
  the authored span count and each known unicode-range expansion before
  appending, avoiding repeated capacity growth while preparing styled input.
- **2026-08-10 — control-free bidi runs.** Stripping LRO/PDF controls now
  retains a run's existing `Rc<str>` when it contains no bidi formatting
  controls, rather than unconditionally allocating a replacement string.
- **2026-08-10 — OpenType position-feature detection.** The all-or-nothing
  comparison of visible source characters to shaped glyphs now streams both
  iterators and detects unequal lengths directly, removing two temporary
  vectors.
- **2026-08-10 — font-support classification.** Classification now examines
  visible glyph IDs in one pass, retaining only boolean summary state instead
  of collecting the IDs before checking outlines and color glyphs.
- **2026-08-10 — selected-face shaping style.** Parley shaping now receives
  a `ParleyStyleView` which borrows the authored `ComputedStyle` and carries
  only selected-face variation settings or synthesis-suppression weight/style
  overrides. Ordinary shaping therefore no longer clones a full computed
  style; descriptor variation settings allocate only when a selected
  `@font-face` actually supplies them. The view is deliberately private to
  the text/Parley boundary: CSS metrics, feature resolution, provenance, and
  general layout continue to consume the authored style directly.
- **2026-08-10 — visual bidi and `unicode-range` style ownership.** Visual
  bidi shaping no longer clones each span's `ComputedStyle`: the temporary
  direction and `unicode-bidi` mutations were not read downstream, while the
  existing LRO/PDF guards remain the real Parley bidi input. Styled
  `unicode-range` selection now uses a `SelectedFaceStyleView` that borrows
  the authored style and owns only a changed effective font family. Font
  selection, face descriptor variations/default features, emoji-family
  choice, and Parley family emission use that effective family; metrics,
  provenance, and all non-face CSS properties continue to use the authored
  style. This removes the previous full-style clone for every selected range.

## Completed organization follow-ups

- **2026-08-10 — text API imports are linted.** Removed
  `#![allow(unused_imports)]` from `src/text/system/api/mod.rs` and removed
  the stale imports and re-exports it concealed. Paint types now live in the
  glyph module that consumes them, and shaping re-exports are limited to the
  sibling-facing helpers that are actually used. `cargo check` is warning-free,
  so future unused imports in this module are reported normally.

## Remaining allocation and copy reductions

These are ordered roughly by expected benefit and implementation confidence.

| Priority | Location | Current cost | Recommended change |
| --- | --- | --- | --- |
| High | `src/text.rs:342-505` | `source_slice` clones selected glyphs and creates new `Rc<str>` for every selected run. Its validation scans `selected_source_ranges` for every source scalar; `source_range_advance_width` rescans every glyph for every source scalar. Repeated fitting candidates become quadratic or worse. | Store authored text as shared backing storage plus byte ranges, not a fresh `Rc<str>` per slice. Build a cluster provenance index with prefix advances once per shaped line; use binary search/linear merge for range validation and width queries. Materialize `String`/`Rc<str>` only at the PDF extraction boundary that needs ownership. |
| High | `src/text/system/api/font.rs:33-61`, `font_matching.rs:517-529` | Every shaping context clones the entire `FontFeatureValues` collection into `FontFeatureContext`. | Store the document-level values behind an immutable `Arc` and let `FontFeatureContext` clone that handle, or lifetime-split the immutable values from the mutable Parley state. Clone only selected face defaults when a caller truly needs them independently. |
| Medium | `src/text/system/api/glyphs/raster.rs:22-109`, `glyphs/color.rs:34-91` | On a run that contains a raster/COLR glyph, every retained ordinary glyph is cloned into a fresh vector because `RenderedGlyphs` is already an `Rc<[RenderedGlyph]>`. | Keep glyphs in an owned staging `Vec` until raster/COLR extraction has partitioned them, then freeze the retained text glyphs into `Rc<[RenderedGlyph]>`. Alternatively add a dedicated mutable paint-partition representation. This removes copying of all non-color/non-raster glyphs in affected runs. |
| Medium | `src/text.rs:687-721` | `rendered_run` allocates `glyph_text: String` just to decide whether `/ActualText` is needed, then allocates/clones the glyph array. | Compare the sequence of glyph Unicode slices to `self.text` incrementally (length plus byte comparison) and stop at first mismatch. This removes the temporary string; the output glyph clone remains necessary while the shaped line is retained. |
| Medium | `src/text/system/api/shaping/normalization.rs:40-56` | `font-variant-emoji: text|emoji` always allocates and builds `output`, even when there is no participating base or every base already has a selector. | Scan until the first insertion is needed, then allocate with capacity and copy the already-scanned prefix. Preserve/make explicit the source map required by the P1 fix. |
| Medium | `src/text/bidi.rs:90-113`, `typographic_units.rs:14-44`, `src/text.rs:954-965` | Grapheme boundaries are collected into a vector solely to walk adjacent boundaries. | Use a streaming previous/current boundary iterator. Retain a vector only where later random access is required. This removes a short-lived allocation from common bidi, typographic-unit, and terminal-tracking work. |
| Medium | `src/text/shaping.rs:36-84` | Direct fallback shaping collects characters into a vector before one pass. | Use a `Peekable` iterator. The total character count is needed only for terminal tracking and can be determined from `next()`/`peek()` without a `Vec`. |
| Medium | `src/text/system/font_loading.rs:685-740` | Fontique query helpers clone every candidate into a vector, then usually stop at the first eligible candidate. | Offer callback/iterator-based `find_font` variants for first-match consumers. Keep the `Vec` API only where both synthesized and non-synthesized candidate passes genuinely require replay. |
| Medium | `src/text/system/font_registry.rs:279-310` | Cache lookups/inserts clone `FontRequest`, which can contain normalized family strings and a list. | Intern normalized family requests or use a borrowed lookup key (`Borrow`/equivalent prehashed key) so a cache hit does not allocate or clone the full request. |
| Low | `src/text/breaking.rs:73-92`, `433-463` | The strict/loose line-break transformation returns an owned `String` even when ICU reports no break, and `text_with_hyphenation_controls` wraps it as `Cow::Owned` unconditionally. | Return `Cow<'a, str>` from the transformation or signal `None` when no U+200B is inserted. This preserves the original borrowed input on the no-op path. |
| Low | `src/text/system/api/shaping/lines.rs:528-588` | Justification allocates/sorts all opportunities and then scans all runs after each selected separator. | If profiling confirms significance, traverse in visual order once and maintain a cumulative run shift; avoid the sort when runs are already ordered. Keep the current ordering rules explicit. |

## Additional organization observations

- **2026-08-10 — shaped artifacts extracted.** Durable `ShapedGlyphRun`,
  `ShapedInlineLine`, `ShapedInlineRun`, and `ShapedInlineGlyph`, together
  with their source-provenance, source-slice, justification, and PDF-run
  conversion operations, now live in `src/text/artifacts.rs`. `text.rs`
  re-exports the artifact types as its compatibility facade.
- **2026-08-10 — CSS Text helpers extracted.** Scalar classification,
  white-space normalization, trimming, and line-end tracking helpers now live
  in `src/text/css_text.rs`; `text.rs` retains the shared re-exports used by
  layout and shaping. Font registry identities remain the next candidate for
  extraction into `text/system/registry_types.rs`.
- `src/text/system/api/mod.rs` no longer suppresses unused-import diagnostics;
  the stale glyph and shaping re-exports have been removed and module-local
  paint imports restored. Its remaining broad imports are a shared scope for
  child implementation modules, so a future organization pass can still
  replace those with explicit child imports where doing so improves ownership
  clarity. Keep the compiler lint enabled while that work proceeds.
- The present `ShapedGlyphRun` -> `ShapedInlineRun` conversion is a useful boundary, but both records carry duplicated text, glyph vectors, font fields, and source data. Name them for their lifetime and ownership instead: e.g. `BackendShapedRun` (Parley conversion, owned glyphs) and `LineRun` (durable layout/PDF artifact). Make provenance a required field whose coordinate system is encoded in its type rather than `Option<Range<usize>>` with an implicit string owner.
- Keep the current separation between CSS opportunity collection and backend shaping. In particular, do not optimize by feeding line-break policy back into Parley; `parley_word_break`'s neutral policy is a good fidelity boundary.
- `FontSystem::clone` copies the font registry and caches for iframe measurement and repeated target-reference passes (`src/text.rs:67-83`, `src/layout/split_1.rs:656`, `src/html.rs:444`). Separate immutable loaded-font state from per-layout mutable scratch/caches. Share immutable font metadata/programs with `Arc`; create a fresh `ShapingSession` only for Parley layout scratch and intentionally pass-local caches. This will make clone cost predictable and reduce repeated cache-map/string cloning without sharing mutable Parley state unsafely.

## Suggested verification plan

1. Add unit tests for mapped provenance invariants: each non-`None` glyph range is contained in the returned line text, falls on character boundaries, and covers the source intended by the cluster.
2. Add emoji-selector plus source-slice tests both before and after an inserted selector, with a following Arabic word to catch loss of contextual forms.
3. Add visual-order guard plus source-slice tests for LTR and RTL resolved ranges.
4. Add malformed raster-dimension and unsupported COLR-operation tests; the latter should assert fallback retention until the full paint graph exists.
5. Benchmark a long, many-span paragraph with `unicode-range`, `font-variant-emoji`, and `font-feature-values`; track allocations and time for shaping, source-range measurement, and source slicing before and after each change.

The central invariant to preserve throughout is: every durable glyph provenance range must index the exact authored text owned by its `ShapedInlineLine`. Representing that invariant directly will both improve correctness and eliminate most of the otherwise tempting, local clone reductions.
