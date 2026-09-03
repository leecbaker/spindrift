# CSS Lists and Counters Parity

Spindrift currently passes **138 of 145 (95.2%)** runnable
`css/css-lists/` WPT reftests. The current result bundle is
`../spindrift-wpt/results/css-lists-counter-final-26/engine-results.json`.

## Implemented counter model

- Counter values use clamped typed arithmetic.
- `counter-reset` supports forward and `reversed()` counter creation. Counter
  membership follows CSS tree-order inheritance (parent or preceding-sibling
  membership with preceding-element values), so an omitted reversed start
  includes mutations in following siblings and descendants until a same-named
  sibling counter shadows that instance. Reversed-start planning tracks this
  contribution scope separately from the inherited runtime counter stack, so
  a nested same-named reset does not absorb its enclosing list's following
  sibling increments.
- Reset, increment, and set ordering follows CSS Lists; duplicate increments
  compound and duplicate resets/sets retain declaration-order semantics.
- The logical counter plan excludes boxes suppressed by `display: none`, gives
  DOM elements stable render origins, and records post-event counter snapshots
  for markers and named strings across normalization, speculative pagination,
  and fragment replay. Page-counter seeds are derived after reversed starts are
  resolved.
- HTML `ol[start]`, `ol[reversed]`, and `li[value]` behavior is expressed as
  user-agent cascade declarations. Author declarations therefore override the
  HTML defaults through the normal cascade.
- HTML `type` attributes use the rendering standard's zero-specificity
  presentational hints: ordered values are case-sensitive on `ol` and `li`,
  while `none`, `disc`, `circle`, and `square` are case-insensitive on `ul`
  and `li`.
- Implicit `list-item` increments follow the active counter direction.

## Implemented marker behavior

- Marker counter properties, generated quote content, nested
  `::before::marker`/`::after::marker` rules, and inherited
  `-webkit-text-fill-color` are supported.
- Direct `::marker` declarations use the regular text-property cascade for
  writing/text direction, spacing, line breaking, text decoration, emphasis,
  and shadows while marker layout properties remain inert.
- The UA marker defaults, including tabular numeral forms, apply to principal,
  `::before`, and `::after` markers.
- Block-level inside markers contribute to the owning list item's intrinsic
  inline sizes. This shared measurement covers tree-abiding generated
  `::before`/`::after` list items and preserves each pseudo's counter snapshot,
  so shrink-to-fit floats reserve the same marker width that final layout
  paints; outside markers remain zero intrinsic contribution.
- Inline flow-root list items retain outside markers; non-atomic inline list
  items use inside marker participation.
- Outside-marker baseline alignment includes half-leading, and vertical inside
  markers contribute their full marker-plus-content inline-axis extent.
- An otherwise empty inline scope with distinct font or line-height metrics is
  retained as a metrics-only first-line strut. It supplies the resolved
  outside-marker baseline without text, advance, decoration, or box paint.
- A horizontal outside marker with no principal line resolves its fallback
  anchor against an adjacent float's shortened line span. This is WPT
  compatibility behavior: CSS Lists leaves exact float-adjacent outside-marker
  placement undefined.
- Pending outside-marker anchors are scoped to their real principal flow;
  atomic-inline captures and speculative off-page measurements cannot consume
  or duplicate an enclosing list item's marker.
- URL-backed `image-set()` markers preserve the selected candidate's intrinsic
  resolution, so their inside and outside marker boxes use the correct CSS
  intrinsic dimensions.
- Pseudo selectors after combinators preserve their implicit universal selector
  (for example, `.list > ::before` routes as `.list > *::before`).
- Generated `content` on an inline flex pseudo is materialized as an anonymous
  flex item, including visible overflow from an authored zero-width container.
- Overridable predefined counter styles resolve through the cascaded UA and
  author `@counter-style` rules, so their descriptors (including range and
  fallback) apply consistently to markers and generated counters.

## Remaining WPT clusters

Seven tests remain:

- Five inline list-item and inline table-fixup tests:
  `inline-block-list{,-marker}`, `inline-list{,-marker}`, and
  `inline-list-with-table-child`.
- One outside string-marker test: `list-style-type-string-005b`.
- One float/width stress test: `list-style-type-string-007`.

The largest remaining architectural gap is preserving inline formatting and
counter scopes through every block-in-inline and table-fixup normalization
shape while producing identical atomic and non-atomic marker geometry. The two
remaining string-marker cases need their float interaction and zero-width
outside-marker geometry reconciled with the shared inline layout artifact.

## Specifications

- <https://drafts.csswg.org/css-lists-3/>
- <https://drafts.csswg.org/css-counter-styles-3/>
- <https://html.spec.whatwg.org/multipage/grouping-content.html#the-ol-element>
- <https://drafts.csswg.org/css-pseudo-4/#marker-pseudo>
