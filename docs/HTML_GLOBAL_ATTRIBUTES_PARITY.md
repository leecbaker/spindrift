# HTML Global Attributes WPT Parity

The renderable `html/dom/elements/global-attributes/` WPT selection passes
39/39 reftests after static `dir=auto` form-control directionality and
comment-aware, atomic `background` shorthand parsing were implemented.

The static renderer uses an input's initial `value` attribute for the HTML
auto-directionality algorithm. Script-driven value changes remain outside the
static-document model.
