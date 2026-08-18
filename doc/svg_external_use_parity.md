# External SVG `<use>` parity

Inline SVG `<use href="URL#fragment">` resolves a statically preloaded,
same-origin external HTML, XML, or SVG document before the SVG scene reaches
`usvg`. Quire selects an SVG-namespace fragment target, imports its owning SVG
fragment into a private `<defs>` payload, namespaces IDs, and rewrites the
external use to a local fragment reference. This preserves static SVG
definitions and allows nested external `<use>` links without parser-time I/O.

Missing, malformed, cross-origin, cyclic, and non-SVG targets paint no shadow
tree, consistently with unavailable visual assets. Resource fetch failure uses
the existing optional visual-subresource policy.

The implementation covers the static WPT cases for HTML-contained symbols,
XML targets outside an SVG root, SVG response MIME parsing, and nested external
`<use>` chains. It does not yet support external SVG `<image>` URLs,
cross-origin/CORS loading, external document-wide CSS, or dynamic updates.

Relevant specifications: [SVG 2 `<use>`](https://www.w3.org/TR/SVG2/struct.html#UseElement),
[SVG 2 URL processing](https://www.w3.org/TR/SVG2/linking.html#processingURL), and
[SVG processing modes](https://www.w3.org/TR/SVG2/conform.html#processing-modes).
