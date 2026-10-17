# The Tahoe shell: what is drawn, and what a screenshot of it is

> The shell's appearance is now a real renderer with real type. This
> document says exactly what that means and where the honest line is.

---

## 1. Screenshots

```sh
cargo run -p tahoe-shell --bin tahoe-preview -- --scene all --out ~/Pictures/tahoe/
python3 tests/png-compress.py ~/Pictures/tahoe/*.png    # 6.4 MB -> ~70 KB each
```

Five scenes: desktop, launcher and control centre in the dark appearance,
desktop and launcher in the light one.

**What they are:** an exact render of the shell's layout, palette, radii and
type, produced by the code that defines them. If the design system changes,
the pictures change with it.

**What they are not:** a photograph of a running compositor. The windows in
them are placeholders, not `wl_surface`s belonging to real clients. That
distinction is the point of this document, because the interesting failures
— a client that never commits a buffer, a seat that delivers no input, a
window that renders underneath the panel — are exactly the failures a render
cannot show.

## 2. The renderer

`tahoe-shell::render` is a small software rasteriser: rectangles, rounded
rectangles with anti-aliased corners, vertical gradients, drop shadows, and
glyphs.

Three decisions worth recording:

**Software, not GPU.** A panel, a dock and three windows are rounded
rectangles and text. There is nothing a GPU would do better here, and a pure
renderer is testable — which is how the shell's layout has 30 assertions
rather than a screenshot somebody looked at once.

**One design system.** The wallpaper is drawn by calling
`DesignSystem::wallpaper_at`, the same function the compositor uploads as a
GL texture and the same one the Swift emitter writes into SwiftUI. Three
renderers, one gradient.

**Text from the system's fonts.** `ab_glyph` (Apache-2.0) parses the
TrueType files already installed; the shell has a *preference order*, not a
font dependency. A missing boldface silently falls back to the regular
face rather than refusing to start, because a desktop that will not start
is worse than one with two weights of the same family.

Anti-aliasing is grayscale coverage. Subpixel rendering assumes an RGB
stripe order the compositor cannot know about, so a compositor-side renderer
should not use it.

## 3. What the shells shows

| Piece | Drawn from |
|---|---|
| Menu bar | `design.panel` over the wallpaper, with the hostname, a clock, workspace pills and shape-drawn status indicators |
| Windows | `design.window_background`, `radii.window`, three traffic lights, a title and placeholder body lines |
| Dock | `design.dock`, `radii.dock`, one tile per app with its initial and a running dot |
| Launcher | A scrim, a search field, rows with name and identifier, accent highlight on matches |
| Control centre | Tiles that **name what they would bind to** — NetworkManager, BlueZ, PipeWire, the Settings portal |

That last row matters. A drawn tile is not a working switch, so each one
says what it would be bound to rather than pretending. The same rule as the
UEL's confidence model, applied to the interface.

## 4. Verified, and how

A picture is not unit-testable, so the checks are structural and the ink
check is the interesting one.

| Check | How |
|---|---|
| The render is not blank | distinct colours in a coarse sample |
| Text is present | **contrast against the region's dominant colour**, not darkness |
| The wallpaper is a gradient | luminance at the top versus the bottom of a clear column |
| The panel strip is distinct | mean luminance of the strip versus the work area |
| The dock is centred | the horizontal span of the non-background band |
| The accent is used | pixels in the accent's range |
| Focus changes the picture | two renders compared |
| Opening a surface changes the picture | ditto, and the scrim darkens what is behind |
| A tiny canvas does not panic | 1×1 up to 200×120, all scenes |
| PNG chunk CRCs are right | recomputed independently |

**The ink check is worth its own note.** The first version counted *dark*
pixels and was wrong in both directions: in the dark theme the panel itself
is dark, so it measured the background and reported 11 166 "text" pixels
that were mostly nothing; in the light theme the muted menu items are too
light to register, so it reported 75. Measuring contrast against the
dominant colour works in both, and would catch a font path that silently
failed — which is the realistic way text disappears from a render.

## 5. PNG, honestly

`Canvas::to_png` writes IDAT using **stored (uncompressed) deflate blocks**.
That is valid PNG, needs no dependency, and makes a 1600×1000 screenshot
6.4 MB. `tests/png-compress.py` re-encodes the same pixels to about 70 KB,
99% smaller, and the losslessness is checked by comparing decompressed
scanlines byte for byte.

A shipping shell would use a real encoder. The reason this one does not is
that the encoder is not the interesting part of a desktop.

## 6. What is not done

- **No `wl_surface`.** The shell does not yet connect to the compositor.
  What exists is the renderer and the model; wiring them to layer-shell is
  the next step, and it is the step that turns these pictures into a
  session.
- **No icons.** Tiles show an initial because there is no icon theme here.
  A drawn approximation would be a worse lie than a letter.
- **No animations.** Every frame is static, which is why the screenshots are
  meaningful at all.
- **The launcher's search is a filter, not an index.** `LauncherIndex` in
  `tahoe-shell` parses `.desktop` files; the scene uses fixed rows.
