# Quire - render HTML as PDF

This is a rust implementation of an HTML layout and rendering tool, aiming for feature parity with `weasyprint`. We should shoot for full compliance with relevant specs, and where there is ambiguity, look to weasyprint for guidance.

This project was called `reasyprint`, and is now called `quire` or `quirepdf`.

## Goals

1. Faithful implementation of relevant HTML/CSS/PDF specs.
   a. Correctness of implementation is top priority
   b. A complete implementation is important, but secondary to that.
2. Compatiblity with Weasyprint input. Less important than spec conformance, but we need to match or exceed what Weasyprint can do. Weasyprint source code is around and can be used as a comparison, but the W3C specs are more important.

## Resources

Prefer the following resources for implementation:

* Relevant HTML, CSS, W3 specs.
* Weasyprint source code is checked out at `Weasyprint/`
* Weasyprint examples at `weasyprint-samples`

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

## Tests

For all tests, the files needed to run it need to be local in the repository. Don't refer on http resources, or on files outside the repo. Don't hardcode local paths.

## Documentation

After each change that modifies the output, let's update documents detailing (1) the current level of parity, and (2) the features needed to reach parity. We can then review that documentation for next steps.
