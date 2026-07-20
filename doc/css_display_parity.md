# CSS Display parity

The `display: contents` implementation now flattens generated pseudo content
with the DOM child stream, prevents the suppressed element from originating
typographic pseudos or box decoration, preserves flattened generated text
through flex/grid itemization, and applies CSS Display Appendix B suppression
before HTML `<br>` handling. Host-CSS `display` also crosses into the supported
inline-SVG scene subset, and `math` parses as the required non-MathML `flow`
fallback.

Fresh `quire-wpt evaluate-test` runs on 2026-07-28 pass these nine screenshot paths:

- `display-contents-before-after-002.html`
- `display-contents-before-after-003.html`
- `display-contents-first-letter-001.html`
- `display-contents-first-letter-002.html`
- `display-contents-first-line-002.html`
- `display-contents-svg-elements.html`
- `display-contents-td-001.html`
- `display-contents-unusual-html-elements-none.html`
- `display-math-on-pseudo-elements-002.html`

`display-contents-fieldset-nested-legend.html`,
`display-contents-flex-002.html`, `display-contents-flex-003.html`,
`display-contents-root-background.html`, and
`display-flow-root-list-item-001.html` still have comparison differences.
The outstanding semantic limits are tracked in `SPEC_DIVERGENCES.md`; the flex
references additionally expose the renderer's ordinary blockified-inline flex
reference path, rather than a flattened-content loss.
