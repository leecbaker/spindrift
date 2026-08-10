# CSS Display parity

The `display: contents` implementation now flattens generated pseudo content
with the DOM child stream, prevents the suppressed element from originating
typographic pseudos or box decoration, preserves flattened generated text
through flex/grid itemization, and applies CSS Display Appendix B suppression
before HTML `<br>` handling. Host-CSS `display` also crosses into the supported
inline-SVG scene subset, and `math` parses as the required non-MathML `flow`
fallback.

Fresh `quire-wpt evaluate-test` runs pass these ten screenshot paths:

- `display-contents-before-after-002.html`
- `display-contents-before-after-003.html`
- `display-contents-first-letter-001.html`
- `display-contents-first-letter-002.html`
- `display-contents-first-line-002.html`
- `display-contents-svg-elements.html`
- `display-contents-td-001.html`
- `display-contents-root-background.html`
- `display-contents-unusual-html-elements-none.html`
- `display-math-on-pseudo-elements-002.html`

`display-contents-fieldset-nested-legend.html`,
`display-contents-flex-002.html`, `display-contents-flex-003.html`,
`display-flow-root-list-item-001.html` still have comparison differences.
The outstanding semantic limits are tracked in `SPEC_DIVERGENCES.md`; the flex
references additionally expose the renderer's ordinary blockified-inline flex
reference path, rather than a flattened-content loss.

`inline-table` is an inline-level atomic table wrapper. Mixed block containers
now classify that outer display role during direct-DOM traversal and anonymous
inline-run normalization, so its table paint remains in source order with
surrounding blocks; `display: table` remains block flow. This follows CSS
Display outer roles, CSS Tables wrapper generation, CSS 2.2 anonymous block
boxes, and CSS Positioned Layout painting order:

- <https://drafts.csswg.org/css-display-3/#outer-role>
- <https://drafts.csswg.org/css-tables-3/#anonymous-boxes>
- <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
- <https://drafts.csswg.org/css-position-4/#painting-order>
