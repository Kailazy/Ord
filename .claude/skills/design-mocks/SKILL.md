---
name: design-mocks
description: Mock a new or reworked UI component on a Claude Design canvas before building it, always with several states and a scale test per design. Use when Kai asks for mocks, design drafts or options for a window, popup, row, sheet or any new feature surface, and before building a feature whose layout is not obvious.
---

# Design mocks on the canvas

A mock that shows only the happy path hides the decisions that matter. Every
design direction is drawn in several states and at scale, side by side, so the
weak ones fail on the canvas and not in egui.

## Where

Use the Artifact tool's Design canvas type (`action: "quickstart"`,
`intent: "design"`, then follow the type's own instructions). One canvas per
feature, titled after the surface ("Pick release mocks"). Give Kai the link as
soon as the first artboard is up and keep adding to the same canvas; never a
local HTML file, never a second canvas for the same feature.

## The grid

Columns are design directions (2 to 3, named `A · …`, `B · …`). Rows are
states, each row under a `title1` note. Every direction gets every row, so a
direction that only works in the default state is visible at a glance.

Rows to draw, at minimum:

1. **Default**: the typical case with real-looking data from the test library.
2. **Empty**: nothing found, nothing selected, nothing loaded. The window
   shrinks or shows one short line; primary actions are disabled.
3. **Scale**: the stress case. A long list that really scrolls (12+ rows),
   the longest plausible title and value so truncation and wrapping show,
   large counts (hundreds, not 3), and every optional control present at once.

Add a row when the surface has it: loading or busy, error, a destructive
confirm, a selection of many, the narrowest and widest window size, each
density setting.

Make the scale row interactive (`is_interactive: true`) with real scrolling
and a clickable selection, driven by a data array in `renderVals()`, so the
list can be flicked through in Play.

## What the mocks must respect

- Only what egui can draw: flat frames with fill and rounding, outlines,
  grids, scroll areas, images, text styles, segmented controls, tooltips. No
  blur, gradients, glows or animation.
- The window's real width range and the screen height cap. Actions stay
  pinned and visible in the scale row.
- The component library: `ui::button`, `chip`, `window`, `menu`,
  `control_row` heights, tokens from `ui/tokens.rs`, Phosphor icons.
- Copy rules: no explanatory captions under headers or buttons, a label is
  the explanation; sentence case; no em dashes; terse tooltips; never the
  word "rekordbox"; the product is Ord. Keep it simple: no counters or
  badges nobody asked for.
- A hover detail (a list of affected songs, say) is drawn open in the scale
  row, capped in height and scrolling, so its worst case is on the canvas.

## After drawing

Reply with the link, one line per direction on its trade-off, and a
recommendation. State which states broke which direction. Apply Kai's
comments to every artboard a change touches, not only the default row.
Build nothing in the app until Kai picks a direction.
