# CSS Lists parity

Automatic marker content is selected from the originating list item's
`list-style-image` and `list-style-type`; `::marker` supplies only marker
styling unless it has explicit `content`. The principal list-item counter event
therefore remains the sole implicit `list-item` increment, and generated
`::before` content observes its post-increment snapshot.

Counter planning records each counter's originating element or tree-abiding
pseudo-element. A `counter-reset` therefore replaces an identically named
counter created by a preceding sibling, while preserving an ancestor-created
counter as an outer `counters()` level. The source-order snapshots used by
generated content follow the same model.

In vertical writing, an inside marker and the principal inline content share a
committed first-line sequence. Its selected line geometry is used for both the
list item's logical-inline extent and paint, including when explicit
`::marker` content uses a different font size from the list item.

For horizontal writing, an outside text marker is baseline-aligned to the
first accepted in-flow line of the list item. This uses normal CSS 2.2
block-in-inline normalization and an inline-block's exported line baseline;
outside images use the same line's block-start edge. CSS Lists intentionally
does not fully prescribe outside-marker geometry, so vertical-writing,
fragmented, and PDF/UA list-label semantics remain tracked in
`SPEC_DIVERGENCES.md`.

Relevant specifications:

- <https://drafts.csswg.org/css-lists-3/#markers>
- <https://drafts.csswg.org/css-lists-3/#creating-counters>
- <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
- <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
