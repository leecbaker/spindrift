# Quire - render HTML as PDF

This is a rust implementation of an HTML layout and rendering tool, aiming for feature parity with `weasyprint`. We should shoot for full compliance with relevant specs, and where there is ambiguity, look to weasyprint for guidance.

This project was called `reasyprint`, and is now called `quire` or `quirepdf`.

## Goals

1. Faithful implementation of relevant HTML/CSS/PDF specs.
   a. Correctness of implementation is top priority
   b. A complete implementation is important, but secondary to that.
2. Compatiblity with Weasyprint input. Less important than spec conformance, but we need to match or exceed what Weasyprint can do. Weasyprint source code is around and can be used as a comparison, but the W3C specs are more important.
3. PDF spec compliance with PDF/A and PDF/UA.
4. For now, external API compatibility is not a goal. We should shoot to have the best goal, and not worry about breaking changes.

## Resources

Prefer the following resources for implementation:

* Relevant HTML, CSS, W3 specs.
* Weasyprint source code is checked out at `Weasyprint/`
* Weasyprint examples at `weasyprint-samples`

Note that we aim to be much more spec compliant and performant than weasyprint. Use it only as a source of checking how they think about things, but don't necessarily copy behaviour without determining it to be the best solution.

## Documents

* `SPEC_DIVERGENCES.md`: An exhaustive list of places that we think we are divergent from the spec. This should not be a log of what was done, but literally where we don't meet the spec. Update whenever you identify a divergence.
* `AGENTS/pdf_comparison.md`: How to compare the output of two PDFs to detect changes, using software already on this computer.

## Development guidelines

* With every significant change, consider if the architecture of the components being modified is appropriate to support the full feature set. For example, when designing a module, ensure the design is appropriate for when we have the full set of features implemented.
* Ensure that `cargo clippy` passes at the end of each change.
* Before adding a crate, the user must approve. Don't avoid asking the user; if it's the best solution, recommend it to the user.
* For functions or structs implementing a feature, cite the relevant W3C specs, or PDF specs in the Rustdoc.
* Once a page is over about 500-1000 lines, then consider breaking it out into a module for better organization.
* When fixing a WPT, don't implement stopgaps; make sure to fix the underlying problem, and do it in the best way.
* WPT fixes and regressions don't need additional smoke tests specific to the WPT test; in most cases, you can just fix the underlying concern and rely on me to run WPT in the future.

## Web platform tests

When fixing a WPT test, or any other individual case, make sure that the fix is the best architectural fix, and fixes the root cause. We want to keep the approach and architecture as pure as possible in a way that makes future changes as easy as possible, and structured to best manage the complexity of rendering HTML.

## Tests

For all tests, the files needed to run it need to be local in the repository. Don't refer on http resources, or on files outside the repo. Don't hardcode local paths.

## Documentation

After each change that modifies the output, let's update documents detailing (1) the current level of parity, and (2) the features needed to reach parity. We can then review that documentation for next steps.

* `SPEC_DIVERGENCES.md`: This should be an exhaustive list of specifics about divergences from the spec. We will use this to guide tasks.
* `doc/*`: Specific implemenation plans, or parity documents, can be in here (one for each specic spec being implemented), but make sure divergences are listed in the central document.
