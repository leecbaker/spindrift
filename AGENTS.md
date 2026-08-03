# Quire - render HTML as PDF

This is a rust implementation of an HTML layout and rendering tool, aiming for feature parity with `weasyprint`. We should shoot for full compliance with relevant specs, and where there is ambiguity, look to weasyprint for guidance.

This project is called `quire`.

## Goals

1. Faithful implementation of relevant HTML/CSS/PDF specs.
   a. Correctness of implementation is top priority
   b. A complete implementation is important, but secondary to that.
2. Compatiblity with Weasyprint input. Less important than spec conformance, but we need to match or exceed what Weasyprint can do. Weasyprint source code is around and can be used as a comparison, but the W3C specs are more important.
3. PDF spec compliance with PDF/A and PDF/UA.
4. For now, external API compatibility is not a goal. We should shoot to have the best goal, and not worry about breaking changes.

## Resources

Prefer the following resources for implementation:

- Relevant HTML, CSS, W3 specs.
- Weasyprint source code is checked out at `Weasyprint/`
- Weasyprint examples at `weasyprint-samples`
- Weasyprint is installed locally via homebrew, you can run it.

Note that we aim to be much more spec compliant and performant than weasyprint. Use it only as a source of checking how they think about things, but don't necessarily copy behaviour without determining it to be the best solution.

## Documents

- `SPEC_DIVERGENCES.md`: An exhaustive list of places that we think we are divergent from the spec. This should not be a log of what was done, but literally where we don't meet the spec. Update whenever you identify a divergence.
- `AGENTS/pdf_comparison.md`: How to compare the output of two PDFs to detect changes, using software already on this computer.

## Coding guidelines

- Use the type system when appropriate to enforce invariants. Consider representation carefully. For example, using euclid-derived types for quantities rather than naked f32.
- Don't be shy to create new enums that best represent the state of something.
- Dont' be shy about using `debug_assert!()` to document preconditions and postconditions of functions.
- Once a file is over about 500-1000 lines of code, then consider breaking it out into a module for better organization. Make sure files are named appropriately for their function.

## Development guidelines

- With every significant change, consider if the architecture of the components being modified is appropriate to support the full feature set. For example, when designing a module, ensure the design is appropriate for when we have the full set of features implemented.
- At the end of each change:
  - Ensure that `cargo clippy` passes with no warnings
  - All tests must pass
  - Format with `cargo +nightly fmt`.
- Before adding a crate, the user must approve. Don't avoid asking the user; if it's the best solution, recommend it to the user.
- For functions or structs implementing a feature, cite the relevant W3C specs, or PDF specs in the Rustdoc.
- When fixing a WPT, don't implement stopgaps; make sure to fix the underlying problem, and do it in the best way.
- WPT fixes and regressions don't need additional smoke tests specific to the WPT test; in most cases, you can just fix the underlying concern and rely on me to run WPT in the future.

## Web platform tests

When fixing a WPT test, or any other individual case, make sure that the fix is the best architectural fix, and fixes the root cause. We want to keep the approach and architecture as pure as possible in a way that makes future changes as easy as possible, and structured to best manage the complexity of rendering HTML.

Tests can be found at `~/projects/quire-wpt/third_party/wpt`.

The latest results for each web platform test are in `~/projects/quire-wpt/results/engine-cache/`. Use those to figure out pass rate for a group or to find tests to work on.

Use `quire-wpt evaluate-test <path>` to render and evaluate one WPT test by its exact path across every configured engine, producing the normal PDFs, raster artifacts, diffs, and report. It is exact selection rather than a prefix filter; add `--include-scripts` only when evaluating a script-driven test. `quire-wpt` can be run with cargo inside `~/projects/quire-wpt/`.

## Tests

For all tests, the files needed to run it need to be local in the repository. Don't refer on http resources, or on files outside the repo. Don't hardcode local paths.

## Documentation

After each change that modifies the output, let's update documents detailing (1) the current level of parity, and (2) the features needed to reach parity. We can then review that documentation for next steps.

- `SPEC_DIVERGENCES.md`: This should be an exhaustive list of specifics about divergences from the spec. We will use this to guide tasks. Only include details about the divergences, and the specs used; don't use it as a log of what is completed.
- `doc/*`: Specific implemenation plans, or parity documents, can be in here (one for each specic spec being implemented), but make sure divergences are listed in the central document.
- `doc/taffy_shortcomings.md`: This document should be a list of limitations, missing features, and bugs in Taffy.

## Design & implementation details

Prefer to use the type system to encode detail, so that you can use the type system to ensure that things are handled and not ignored. For example, see the CSS sizing quantities below.

### CSS Sizing Quantities

Quire should model CSS layout sizes with two separate concerns:

1. The box-model space of the numeric value.
2. Whether the value is definite for CSS percentage resolution.

Use the existing euclid-backed semantic quantities for box-model space:

- `ContentBoxLength`
- `BorderBoxLength`
- `NonContentLength`
- `LayoutLength`

Do not replace these with generic raw scalars. Convert between content-box,
border-box, and non-content values only through the helpers in `src/units.rs`.

### Definite Percentage Bases

CSS definiteness is a layout-time concept used mainly to decide whether
percentages can resolve. It should be modeled at percentage-basis boundaries,
not threaded through all computed CSS values.

Prefer a type like:

```rust
enum PercentageBasis<T, Source = ()> {
    Definite { value: T, source: Source },
    Indefinite,
}
```

### Semantic types

Use types to make invalid layout calculations difficult or impossible. A value’s type should tell the reader:

- What coordinate system it uses.
- Whether it is a position, signed displacement, or non-negative extent.
- Which box-model space it belongs to.
- Whether it is a physical or logical axis.

For example, a content-box width, a border-box width, a page position, and a Flex main-axis size may all be numerically 20.0, but they are not safely interchangeable.

#### Reuse an existing type when the meaning matches exactly

Prefer the established semantic types:

- `ContentBoxLength`, `BorderBoxLength`, `NonContentLength`, `MarginBoxLength`
- `LayoutLength`
- `PhysicalContentWidth`, `PhysicalContentHeight`
- `LogicalInlineContentSize`, `LogicalBlockContentSize`
- `PageInlineSpan`, `PageBlockSpan`, `PageTopPoint`, `PageTopBlockPosition`
- `PaintSize`, `PaintPoint`, `PaintStrokeWidth`

Do not create a new wrapper merely because a variable is called width; create or reuse a type based on what the value means.

#### Make a new type when it prevents a real category error

Create a narrowly scoped type when values can otherwise be confused despite sharing units.
Good examples:

- Flex main size vs Flex cross size.
- A signed margin-inclusive Flex length vs a non-negative resolved Flex size.
- Vertical vs horizontal baseline offsets.
- A source-local Grid offset vs a page-top position.
- A CSS gap-rule width vs a final paint stroke width.

Avoid generic names such as `Width`, `Size`, or `Length` when they hide the relevant distinction.

#### Model invariants in operations

The API should make valid operations easy and invalid operations unavailable.

- A non-negative extent may add to another extent.
- Subtracting two sizes should produce a signed length, not silently clamp.
- Converting a signed length to a size should require an explicit operation such as non_negative_size().
- Subtracting positions should yield a displacement, not a size.
- Moving a position by a size or signed displacement is valid; adding two positions is not.

Avoid convenience methods that undo type safety, such as a generic `project_to_axis<T>()`.

#### Make conversions named and local

Crossing a semantic boundary should use a named conversion that explains why it is valid:

- Physical content box → Flex main or cross size.
- Intrinsic gap → Flex main or cross gap.
- Flex main size + aspect ratio → Flex cross size.
- Typed Flex baselines → scalar Taffy metrics.
- Margin-box child extent → parent block-stack extent.

Keep `.points()` extraction inside these adapters or true backend boundaries such as Taffy, CSS scalar resolution, PDF operators, and paint rectangles. Callers should pass typed values rather than extract and reconstruct scalars.

#### Keep physical, logical, and local coordinates separate

Do not use a physical width where a logical inline size is required, or a page coordinate where a Grid/Flex source offset is required.

Writing-mode projection is a real conversion step. Put it in one named helper rather than repeating `if row { width } else { height }` throughout callers.

#### Treat box-model conversion as explicit

Use helpers in `src/units.rs` to cross content-, border-, non-content-, and margin-box spaces. Do not manually add padding and borders at arbitrary call sites.

If an operation combines values with different box-model meanings, stop and identify the intended result before choosing a type.

#### Keep scalar values only when they are genuinely scalar

Raw f32 is appropriate for:

- Ratios and aspect ratios.
- CSS numeric factors such as flex grow/shrink inputs.
- Counts, indices, tolerances, and epsilon comparisons.
- Local arithmetic inside one named adapter for an untyped external API.

Even then, wrap values when their role is easy to confuse—for example, Flex grow factor versus shrink factor, or a grow fraction versus a shrink fraction.

#### Prefer small composites over related scalar pairs

If several fields jointly describe one concept, make a composite:

- A placement: origin plus available span.
- A pattern tiling: tile size, repeat step, origin.
- A baseline estimate: vertical and horizontal first/last pairs.
- A Flex/Grid available-space record with typed physical dimensions and percentage bases.

This prevents callers from recombining unrelated `x`, `width`, `top`, and `height` values.

#### Review checklist

Before adding or changing geometry code, ask:

1. Is this a position, a signed displacement, or a non-negative extent?
2. Which coordinate system and axis does it use?
3. Which box-model space does it represent?
4. Does an existing type express that exact meaning?
5. If not, could this value be confused with another value today?
6. Can the operation be expressed as a named conversion instead of .points() arithmetic?
7. Does the type preserve CSS correctness in vertical writing modes and percentage resolution?
8. Is raw scalar extraction confined to a real external or legacy boundary?

The goal is not to eliminate every f32. It is to make the important semantic boundaries visible and enforceable, so future code cannot accidentally mix incompatible layout metrics.
