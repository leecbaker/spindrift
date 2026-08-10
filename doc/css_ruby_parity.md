# CSS Ruby parity

## Current foundation

Quire recognizes the CSS Display ruby values (`ruby`, `ruby-base`,
`ruby-text`, `ruby-base-container`, and `ruby-text-container`) and preserves
them in computed display values. The HTML user-agent stylesheet maps the
semantic ruby elements to those roles, applies the Level 1 annotation font and
nowrap defaults, and suppresses `rp` fallback parentheses. The required ruby
bidi isolation is deliberately deferred with the rest of the ruby bidi model.

Ruby and its layout-internal roles are non-atomic inline participants. This
keeps their base-side DOM subtree available to inline collection, avoids
treating an inline ruby container as an `inline-block`, and preserves normal
out-of-flow/blockification handling for descendants. A typed normalization
pass records explicit roles, anonymous generated segments, and source
intra-level whitespace independently before it groups base segments,
annotation levels, empty counterparts, and spanning annotations. A single
anonymous annotation spans a segment, while a single explicit `rt` pairs only
with its corresponding base; later bases receive anonymous empty annotations.
Only source whitespace between authored same-level role boxes gains an empty
counterpart; generated `content: " "` remains ordinary ruby content. Generated
role ordering and improper-parent anonymous wrappers still need complete
box-tree fixup. Empty annotation levels are trimmed, and positioned or floated
descendants do not manufacture anonymous annotation boxes. During box-tree normalization,
direct in-flow block children of ruby roles are inlinified to inline flow-root
atoms *before* CSS Display's ordinary block-in-inline splitting pass; this
preserves the ruby formatting context instead of converting it into unrelated
anonymous block runs.

The inline collector materializes normalized base columns with annotation
sidecars. The normalization preserves the computed style of each `rbc` and
`rtc` independently from its `rb` and `rt` segments, so container-level
`ruby-overhang` policy is not guessed from annotation text. They are painted separately from the parent text stream, so
annotations cannot become parent text, justification opportunities, or
first-letter candidates. An empty annotation container whose only descendants
are floats or positioned boxes is ignored for normal annotation content
collection. `::first-letter` can style the first base-side word collected from
a ruby container rather than selecting an annotation word. The materializer
measures paired base and annotation levels with distinct typed column and ink
spans, applies `ruby-align: start | center | space-between | space-around`, and
stacks annotations on a typed logical over/under side. `ruby-position:
alternate | over | under` is inherited and the originating block's selected
`::first-line` overlay is replayed into captured ruby base and annotation
records; descendant `ruby::first-line` and `rt::first-line` rules do not
create a second scope. Ruby role styles carry a used transform-applicability
decision, preventing transform paint and geometry from being established for
Ruby's non-transformable structure. Relative ruby and ruby-base-container
scopes retain their positioned descendants' containing-block ownership.

After CSS Text has selected a parent line, the materializer resolves ruby
overhang against its immediate *visual* neighbors. The resulting placement is
attached only to the selected inline atom, never to the reusable normalized
ruby source. `ruby-overhang: spaces` (and legacy `none`, which aliases it)
borrows preserved spaces or tabs, U+00A0 and Unicode `Zs` separators, plus the
applicable untrimmed fullwidth punctuation share. `auto` uses Quire's
documented deterministic UA policy: it may borrow at most `0.5ic` on each
logical side from an immediately adjacent non-atomic visual text item. Any
unborrowed annotation excess remains in the ruby's normal-flow advance. Paint
uses the line-local logical placement for horizontal interlinear ruby. The
remaining vertical ruby paint projection is tracked below as unfinished work.

Relevant specifications:

- <https://drafts.csswg.org/css-ruby-1/>
- <https://drafts.csswg.org/css-display-3/#layout-internal>
- <https://drafts.csswg.org/css-pseudo-4/#first-letter-pseudo>

## Remaining Level 1 work

The collector still represents annotated columns with a coupled inline atom;
it carries only each base's source text into the parent opportunity graph, so
UAX #14 can govern breaks between paired bases without exposing annotations.
It still needs a true base-level opportunity range for complete fragmentation
and line-metric behavior; the structural `ruby-line-breaking-001` WPT retains
a small metric mismatch. Generated-role whitespace pairing, improper-parent
anonymous wrappers, multi-level span sizing, and vertical-writing-mode
placement also need dedicated conformance work.

Deliberately deferred after this foundation: `ruby-position: inter-character`,
`ruby-merge`, auto-hide/collapse, a glyph-ink collision analysis richer than
the deterministic `0.5ic` `auto` policy, full ruby-specific bidi isolation and
reordering, complete vertical ruby paint projection, and breaks within a
base/annotation pair.
