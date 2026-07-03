# PDF Font Embedding

Last updated: 2026-06-28

Quire emits shaped text through PDF Type 0 fonts with Identity-H CIDs. The PDF
objects are serialized with `pdf-writer`; OpenType font programs are subsetted
before embedding with `fontcull` where that can be done without changing the
glyph IDs already produced by shaping.

## Current Behavior

- Only fonts used by emitted, visible painted text runs are embedded.
- Duplicate document font records that share the same font program and face are
  merged into one embedded PDF font.
- Subsets retain original glyph IDs, so content streams, `/CIDToGIDMap
  /Identity`, `/W`, and `/ToUnicode` all continue to refer to the shaped glyph
  IDs stored on rendered text runs.
- Actual subset fonts use a six-uppercase-letter subset prefix in `/BaseFont`
  and `/FontName`. Full standalone fallback fonts keep the sanitized original
  PostScript name without a subset prefix.
- CID fonts emit an explicit `/DW`, real font bounding boxes, richer descriptor
  metrics, and PDF font flags derived from OpenType metadata where available.
- The font audit path checks OS/2 embedding and subsetting permissions and logs
  default-profile warnings for fonts whose outline embedding is restricted.
- If subsetting fails, does not shrink the font, or cannot be validated, Quire
  embeds the full original standalone font program for that used font only.

## Supported And Fallback Cases

- Standalone TrueType/OpenType sfnt data with face index `0` is eligible for
  subsetting.
- TTC/OTC collection faces are extracted to standalone sfnt programs before
  subsetting or fallback; raw collection bytes are not treated as valid PDF font
  streams.
- Unsupported font programs and invalid subset outputs use full-font fallback
  only when a standalone font program is available.
- The output currently prioritizes shaping correctness and text extraction over
  maximum PDF size reduction; dense CID/GID remapping remains future work.

## Conformance Profiles

- The default PDF variant is PDF/A-2b, which uses a PDF 1.7 header and the
  PDF/A font-planning profile.
- The internal font planner has profile hooks for strict PDF, PDF/A, and PDF/UA.
  PDF/A and PDF/UA font plans attach `/CIDSet` streams for subset CIDFonts, but
  tagged PDF structure, output intents, and validator integration are still
  future work.

## Remaining Work

- Broaden collection extraction coverage with repo-local TTC/OTC fixtures rather
  than relying on optional system fonts.
- Audit CFF, CFF2, bitmap/color fonts, and variable font instances against PDF
  embedding requirements.
- Add dense subset remapping only when content strings, widths, ToUnicode, and
  CID-to-GID maps can be updated as one validated unit.
- Extend the font embedding model for remaining PDF/A and PDF/UA requirements
  alongside tagged PDF work, output intents, and external validator coverage.
