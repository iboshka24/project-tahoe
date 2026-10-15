# COSMIC theming: restyling without forking

> How Tahoe makes an existing, maintained compositor look like Tahoe, and
> precisely where that stops working.
>
> Every field name, path and literal form below was read from upstream
> (`pop-os/libcosmic`, `pop-os/cosmic-panel`, `pop-os/cosmic-settings`) on
> 2026-10-15, not guessed. A wrong RON key is worse than a missing feature,
> so the tests check the emitted keys against the verified upstream list.

---

## 1. The decision

**Patch COSMIC? No. Theme it.** COSMIC keeps its entire appearance in RON
files addressed by config id, and reads system-wide defaults from
`/usr/share/cosmic/`. Restyling is therefore a *file installation* problem.

Forking would mean owning a compositor — security fixes, protocol updates,
the whole maintenance load — to change a palette. That is the same trade
`LICENSING.md` §3 refuses for Wine, and it is refused here for the same
reason.

It is also **not an either/or** with `tahoe-compositor`. That crate remains
the long-term plan; this is the shortest path to a *visibly Tahoe* desktop
today, on someone else's maintained code.

## 2. How the theme is stored

| | |
|---|---|
| Root | `$XDG_CONFIG_HOME/cosmic/` for the user, `/usr/share/cosmic/` for defaults |
| Layout | `<ConfigId>/v<version>/<field>` — **one file per struct field** |
| Format | RON, pretty-printed |
| Version | 2 for a theme, 1 for the mode and the panel |

Three details are easy to get wrong and all three are pinned by tests:

1. **Colours are quoted hex strings.** `base: "#9E9E9E40"`. An unquoted
   `#RRGGBBAA` is not a RON value at all — the theme would fail to parse and
   silently fall back to the compiled defaults. This was a real bug in the
   first version of the generator.
2. **Fixed-size arrays serialise as tuples.** `radius_xs: (4.0, 4.0, 4.0, 4.0)`,
   not `[4, 4, 4, 4]`, and with decimal points because the fields are `f32`.
3. **Optional colours carry their `Some`.** `window_hint` and `shade` are
   `Option<Srgb>`.

The colour type is `palette::Srgba` throughout, serialised through
`cosmic_theme::color_serde`.

## 3. What the generator emits

```sh
cargo run -p tahoe-shell --bin tahoe-cosmic-theme -- --list      # preview
cargo run -p tahoe-shell --bin tahoe-cosmic-theme -- --out build/cosmic
cargo run -p tahoe-shell --bin tahoe-cosmic-theme -- --apply     # current user, live
```

Mapping from `tahoe-shell::design`, so the COSMIC desktop, the Tahoe
compositor and the SwiftUI application all read one palette:

| Tahoe | COSMIC |
|---|---|
| `accent` | `accent`, `accent_text`, `link_button`, the `success`/`warning`/`destructive` hues |
| `window_background` | `background.base`, `primary.base` |
| `panel` / `dock` | `secondary.base` |
| `text` | `background.on`, `control_tint`, `text_tint` |
| `separator` | `background.divider`, `divider` |
| `radii` | `radius_xs` … `radius_l` |
| `spacing` | `space_xxxs` … `space_xxxl` |
| `panel.a < 1.0` | `frosted_panel`, `frosted_system_interface` |

The 32-entry palette is **derived**, not invented: neutrals from the theme's
own background-to-text axis, the accent family by rotating the accent's hue,
which keeps a COSMIC launcher from looking borrowed. A test asserts the
neutral ramp is monotonic.

The panel layout ships as a top panel plus a bottom dock, with the dock's
radius and spacing coming from the design system.

## 4. What a user or a distro does

**A distro:** install the tree into `/usr/share/cosmic/`. Every user gets it
as a default, and a user who changes something keeps their change.

**One user:** `--apply` writes to `~/.config/cosmic/`, which COSMIC
**watches** — the appearance changes without a restart.

Precedence is per field: the user file wins, the system file is the
fallback, the compiled default is the fallback after that. So overriding one
colour is one file with one line.

## 5. What cannot be changed without forking

Verified by reading the named files. None of these is reachable from a
config key, and this is stated rather than quietly ignored:

| Hard-coded | Where |
|---|---|
| Animation durations (200 ms, 320 ms, the workspace and tiling schedules) | `cosmic-comp/src/shell/mod.rs`, `shell/layout/{tiling,floating}/mod.rs`, `shell/element/stack/tabs.rs` |
| The focus-indicator corner radius (`5.0`, a literal) | `cosmic-comp/src/shell/element/mod.rs` |
| Panel applet icon sizes and padding per `PanelSize` | `cosmic-panel-config/src/panel_config.rs` |
| How `Theme` derives hover/pressed/disabled shades from a base colour (the alpha factors in `steps.rs`/`derivation.rs`) | `cosmic-theme`, `libcosmic/src/theme/style/*` |

So a theme can change every colour, radius, spacing and material, and it
cannot change how COSMIC animates or how it computes a shade from a colour
you give it. For a Tahoe look that is enough; for Tahoe *motion* it is not,
and `tahoe-compositor` is where that would live.

**Not verified:** the launcher's and applets' internal spacing.
`cosmic-launcher` and `cosmic-applets` are separate repositories that were
not audited. Do not assume they are configurable.

## 6. What is verified here

| | |
|---|---|
| The generator emits 100+ files under `/usr/share/cosmic` | `tests/check-all.sh` |
| Every panel key is a real `CosmicPanelConfig` field | 1 test — panel structs set `deny_unknown_fields`, so a typo makes the panel refuse to load |
| Colours are quoted, 8 hex digits, opaque where they should be | 3 tests |
| Radii, spacing and the accent come from the design system | 4 tests |
| The neutral ramp is ordered; hue rotation preserves lightness | 3 tests |
| All 39 upstream theme fields are emitted | 1 test |

**Not verified:** no COSMIC session was run against this theme. COSMIC is
not installed on the development machine, so every claim here is about the
files produced, not about a desktop anyone has seen.
