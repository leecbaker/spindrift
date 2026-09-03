# PDF Font Embedding

Last updated: 2026-07-17

Spindrift emits shaped text through PDF Type 0 fonts with Identity-H CIDs. The PDF
objects are serialized with `pdf-writer`; TrueType and OpenType/CFF programs
are subsetted before embedding with `subsetter`.

## Current Behavior

- Only fonts used by emitted, visible painted text runs are embedded.
- CSS faces that select the same physical font program and collection face are
  merged into one embedded PDF font, with their painted glyph usage unioned
  before subsetting. PDF font names, descriptor metrics, and flags come from
  the embedded OpenType program rather than CSS request labels.
- Subsets remap the shaped source glyph IDs to deterministic, dense CIDs.
  Content streams, `/W`, `/ToUnicode`, and PDF/A `/CIDSet` use those CIDs.
  For TrueType, subsetter makes remapped GIDs equal CIDs, so `/CIDToGIDMap
  /Identity` remains correct.
- A shaped run whose individual glyph records cannot express its logical
  source cluster is emitted in a marked-content `/ActualText` span. The span
  preserves authored join controls and one-to-many/many-to-one shaped text for
  extraction, while per-CID `/ToUnicode` fallback entries retain complete font
  coverage. Synthetic shaping context is never included in `/ActualText`.
- Actual subset fonts use a six-uppercase-letter subset prefix in `/BaseFont`
  and `/FontName`. Full standalone fallback fonts keep the sanitized original
  PostScript name without a subset prefix.
- CID fonts emit an explicit `/DW`, real font bounding boxes, richer descriptor
  metrics, and PDF font flags derived from OpenType metadata where available.
- The font audit path checks OS/2 embedding and subsetting permissions and logs
  default-profile warnings for fonts whose outline embedding is restricted.
- System CSS generic-family selection excludes candidates whose OS/2 `fsType`
  forbids outline embedding before shaping. Explicit named system fonts and
  `@font-face` sources remain authoritative and are rejected by the audit when
  their selected program cannot be embedded.
- If subsetting fails, does not shrink the font, or cannot be validated, Spindrift
  embeds the full original standalone font program for that used font only.

## Supported And Fallback Cases

- TrueType and OpenType/CFF data are eligible for subsetting. CFF subset SFNTs
  are reduced to their CFF program and embedded as `CIDFontType0C` streams.
  CFF output is accepted only when that final stream is smaller than the
  original CFF program.
- TTC/OTC faces are passed to subsetter with their face index. Full-font
  fallback extracts the selected face to a standalone program; raw collection
  bytes are not embedded.
- Unsupported font programs and invalid subset outputs use full-font fallback
  only when a standalone font program is available.
- Unsupported programs, including CFF2, retain the existing full-font fallback
  or strict-profile rejection behavior.

## Conformance Profiles

- The default PDF variant is regular PDF, which uses a PDF 1.4 header and the
  default font-planning profile. PDF/A-1b is available explicitly through
  `--pdf-profile pdf/a-1b`.
- The internal font planner has profile hooks for strict PDF, PDF/A, and PDF/UA.
  PDF/A and PDF/UA font plans attach `/CIDSet` streams for subset CIDFonts, but
  tagged PDF structure, output intents, and validator integration are still
  future work.

## Remaining Work

- Broaden collection extraction coverage with repo-local TTC/OTC fixtures rather
  than relying on optional system fonts.
- Audit CFF2 and variable font instances against PDF embedding requirements.
  Bitmap OpenType glyphs are painted as PDF image XObjects rather than passed
  to the Type 0 outline-font embedding path; SVG-in-OpenType glyphs remain
  unimplemented.
- Extend the font embedding model for remaining PDF/A and PDF/UA requirements
  alongside tagged PDF work, output intents, and external validator coverage.
