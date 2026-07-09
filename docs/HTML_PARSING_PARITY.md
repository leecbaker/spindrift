# HTML and XML Parsing Parity

## Current behavior

Quire parses HTML with `html5ever` and XML with `xml5ever` before converting
the result into its internal DOM. Character references are resolved by the
respective parser exactly once in text nodes and attribute values. Later DOM,
layout, resource, and generated-content processing consumes those parsed
values directly.

CSS source is parsed as CSS: HTML character references are not decoded in CSS
strings or URLs, while CSS escapes continue to use CSS parsing rules.

## Remaining work

HTML parser integration, DOM construction, and the HTML-to-PDF metadata mapping
need broader conformance coverage. Known concrete gaps belong in
`SPEC_DIVERGENCES.md` rather than this parity summary.
