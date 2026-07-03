# CSS Block Layout Parity

This note tracks normal-flow block layout behavior from CSS 2.2, especially
where block sizing interacts with margin collapse.

## Current Support

- Adjacent in-flow block sibling vertical margins collapse using CSS 2.2's
  adjoining-margin rules, including mixed positive/negative margin sets.
- First-child top margins and last-child bottom margins can collapse through
  auto-height block containers when no border, padding, clearance, line box, or
  formatting-context boundary separates the margins.
- Self-collapsing empty block boxes keep adjoining parent and sibling margin
  sets open when their own height, min-height, border, padding, and in-flow
  contents allow it.
- A last child's bottom margin is measured outside the parent when it collapses
  through, but is excluded from the parent's min/max-height constraint
  calculation when `min-height` or `max-height` prevents that collapse.
- Forced page breaks between block descendants continue at the next page's
  block-start edge without cloning ancestor block-start margin, border, or
  padding, matching `box-decoration-break: slice`.
- Same-page non-positioned `overflow:hidden`/`clip` block containers clip
  descendants to the used padding box, including auto-height blocks whose
  block-end clip edge is only known after child layout.
- Normal-flow block formatting context roots next to active floats avoid by
  border box, not margin box. Auto-width roots can narrow to the float-free
  band without horizontal margins forcing placement below the float.
- Inline-block shrink-to-fit sizing resolves percentage-height replaced
  descendants against the inline-block's own definite content height, including
  inline runs wrapped in anonymous blocks.
- Raster image natural dimensions are converted from source image pixels to CSS
  px, then into Quire's PDF-point layout unit, before block replaced-element
  sizing uses them.

## Remaining Gaps

- `margin-trim: block-end`, inline-axis margin trimming, and fully logical
  writing-mode-aware margin trimming remain incomplete; see
  `SPEC_DIVERGENCES.md`.
- Fragmented block layout still needs broader WPT coverage for margin collapse
  across page breaks, clearance, and nested formatting contexts.
