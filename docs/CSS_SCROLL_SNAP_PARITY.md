# CSS Scroll Snap WPT Parity

## Current status

The local non-script `css/css-scroll-snap/` WPT selection executes 14 visual
reftest/crashtest candidates. The current implementation passes 12 / 14.

Quire now computes CSS Scroll Snap Level 1 container, area, and offset
properties (`scroll-snap-*`, `scroll-padding-*`, and `scroll-margin-*`) and
has a static snapport/candidate-selection model, captured scroll-content paint
translation and clipping, static iframe browsing contexts, and fragment-target
navigation. The remaining failures are limited to two atomic/replay paint cases
and are listed in `SPEC_DIVERGENCES.md` rather than treated as WPT-specific
exceptions.

## Remaining work

- Finish atomic inline and writing-mode replay so that captured descendant
  backgrounds, overflow clips, and container decoration retain their correct
  paint bands after a static snap translation. The remaining WPT failures are
  `scroll-snap-initial-layout-000.html` and
  `scroll-snap-writing-mode-000.html`.
- Live DOM scrolling, JavaScript, scroll-snap events, and the script-bearing
  WPT cases remain outside Quire's static PDF renderer scope.
