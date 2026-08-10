# Table Fragmentation Parity

Separated-border table geometry retains one leading and one trailing logical
block-axis `border-spacing` edge in the unfragmented source grid. Destination
fragments expose only their committed row slices; internal and fragment-local
spacing gaps are not structural background clips.

Fragmented table structural backgrounds use a shared source-grid projection:
the table, column, column-group, row, and row-group layers resolve their
positioning areas once against the unfragmented logical grid, then project
visible originating-cell slices into the current fragmentainer. Each
projection retains distinct source and destination row slices, so a source
row offset cannot displace the destination table origin. The source clip
retains rowspans and partial row pieces while the destination clip omits
separated-border gaps. Background images and gradients therefore keep one
phase across adjacent fragments, including vertical writing modes.

The destination adapter keeps vertical block origins logical: `vertical-rl`
and `vertical-lr` fragments advance over the fragmentainer's physical block
span without reusing a physical `y` cursor. `box-decoration-break: slice`
continues to use the whole source box; clone behavior selects the fragment
positioning area explicitly.

The exact fragmented table-paint matrix is not yet at parity. The twelve
reported horizontal-tb LTR, vertical-lr RTL, and vertical-rl RTL cases for
table/column/row/row-group backgrounds (including the horizontal grid
control) still differ. The common remaining issue is wrapper/chrome replay
through multicolumn fragmentainers: continuation slices must retain one
continuous wrapper decoration and caption placement while exposing only their
destination-local grid visibility. This is tracked centrally in
`SPEC_DIVERGENCES.md`; no claim of complete writing-mode parity is intended.

Collapsed-border painting for root-level `inline-table` elements also follows
the inline-table's atomic source position during mixed block-flow traversal.
Both direct-DOM classification and formatting-tree normalization distinguish
the inline outer display role from the table inner display type; neither moves
the table-local `TableCollapsedBorder` band nor changes collapsed-border
conflict resolution. Remaining failures in the 13-case painting-order family
are therefore independent positioned or raster-edge issues.

Absolutely positioned table roots use a separate one-shot sizing contract:
the positioned containing block supplies the table's logical inline available
size, while an authored definite logical block size is consumed by table row
distribution before the positioned inset equation places the result. Nested
tables do not inherit this contract, and flex/grid wrapper-size overrides
remain independent. This follows CSS Tables' auto-width and table-height
algorithms together with CSS Position's definite available-space rules.
