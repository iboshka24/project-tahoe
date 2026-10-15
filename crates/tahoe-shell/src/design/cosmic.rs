// SPDX-License-Identifier: MIT OR Apache-2.0
//! Emit a COSMIC desktop theme from the Tahoe design system.
//!
//! # Why this is a generator and not a patch
//!
//! COSMIC keeps its entire appearance in **RON files addressed by config
//! id**, one file per struct field, and reads system-wide defaults from
//! `/usr/share/cosmic/<ConfigId>/v<version>/<field>`. A distribution
//! overriding the look is therefore doing exactly what upstream designed
//! for: writing files. Forking COSMIC to change a palette would mean owning
//! a compositor forever to accomplish what `install -Dm644` accomplishes.
//!
//! Every field name, path component and literal form below was read from
//! upstream (`pop-os/libcosmic`, `pop-os/cosmic-panel`,
//! `pop-os/cosmic-settings`), not guessed. Two details in particular are
//! easy to get wrong and are pinned by tests:
//!
//! - **Colours serialise as `"#RRGGBBAA"`**, eight hex digits, not as an
//!   `(r, g, b, a)` tuple. `Srgb` becomes `FF` alpha.
//! - **Corner radii and spacing are per-field structs**, and RON writes
//!   fixed-size arrays as *tuples*: `(4.0, 4.0, 4.0, 4.0)`, not `[4, 4, 4, 4]`.
//!
//! Panel configs additionally set `deny_unknown_fields`, so a misspelled
//! key is a hard error rather than a silent default. The panel keys emitted
//! here are checked against the verified upstream list in the tests.
//!
//! # What this deliberately does not do
//!
//! Animation durations, the indicator radius, the panel applet size tables
//! and the theme→widget shade algebra are compiled into COSMIC's Rust. They
//! are named in `docs/cosmic-theming.md` as things a distro cannot reach
//! without forking, rather than being quietly ignored here.

use std::fmt::Write as _;

use super::{DesignSystem, Rgba};

/// Version component of a COSMIC config path, from `#[version = 2]` on
/// `cosmic_theme::Theme`.
pub const THEME_VERSION: u32 = 2;

/// Version on the theme *mode* config, from `#[version = 1]`.
pub const MODE_VERSION: u32 = 1;

/// Version on panel configs.
pub const PANEL_VERSION: u32 = 1;

/// The dark theme config id.
pub const DARK_THEME_ID: &str = "com.system76.CosmicTheme.Dark";
/// The light theme config id.
pub const LIGHT_THEME_ID: &str = "com.system76.CosmicTheme.Light";
/// The mode config id.
pub const THEME_MODE_ID: &str = "com.system76.CosmicTheme.Mode";
/// The panel registry config id.
pub const PANEL_ID: &str = "com.system76.CosmicPanel";

/// Prefix a distribution installs under. `find_data_file` on
/// `BaseDirectories::with_prefix("cosmic")` resolves this.
pub const SYSTEM_PREFIX: &str = "/usr/share/cosmic";

/// Render `Srgba` the way `cosmic_theme::color_serde` writes it.
///
/// **Quoted**, and always eight hex digits. The deserialiser accepts a hex
/// *string* or a struct; upstream's own schema files use the string form
/// (`base: "#9E9E9E40"`). Emitting a bare `#RRGGBBAA` was a real bug — that
/// is not a RON value, so every colour file would have failed to parse and
/// the whole theme would have silently fallen back to the defaults.
#[must_use]
pub fn ron_color(c: Rgba) -> String {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "\"#{:02X}{:02X}{:02X}{:02X}\"",
        byte(c.r),
        byte(c.g),
        byte(c.b),
        byte(c.a)
    )
}

/// Render an `Option<Srgb>`: `Some("…")` or `None`.
///
/// `window_hint` is an option upstream; writing a bare colour there loses
/// the `Some` and the option cannot be parsed.
#[must_use]
pub fn ron_optional_color(c: Option<Rgba>) -> String {
    match c {
        Some(c) => format!("Some({})", ron_color(c)),
        None => "None".to_owned(),
    }
}

/// Render a `[f32; 4]` the way RON does: as a tuple.
#[must_use]
pub fn ron_quad(v: f32) -> String {
    let f = super::swift::swift_float(v);
    format!("({f}, {f}, {f}, {f})")
}

/// One file in a theme tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeFile {
    /// Path relative to [`SYSTEM_PREFIX`].
    pub path: String,
    /// File contents.
    pub body: String,
}

impl ThemeFile {
    fn new(path: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            body: body.into(),
        }
    }
}

/// The twelve colours of a COSMIC `Component`.
fn component(
    base: Rgba,
    hover: Rgba,
    pressed: Rgba,
    on: Rgba,
    divider: Rgba,
    accent: Rgba,
    border: Rgba,
) -> String {
    format!(
        "(\n    base: {},\n    hover: {},\n    pressed: {},\n    selected: {},\n    selected_text: {},\n    focus: {},\n    divider: {},\n    on: {},\n    disabled: {},\n    on_disabled: {},\n    border: {},\n    disabled_border: {},\n)",
        ron_color(base),
        ron_color(hover),
        ron_color(pressed),
        ron_color(hover),
        ron_color(accent),
        ron_color(accent),
        ron_color(divider),
        ron_color(on),
        ron_color(base.with_alpha(0.5)),
        ron_color(on.with_alpha(0.65)),
        ron_color(border),
        ron_color(border.with_alpha(0.5)),
    )
}

/// A COSMIC `Container`: the wrapper around a `Component`.
fn container(base: Rgba, on: Rgba, divider: Rgba, component_body: &str) -> String {
    format!(
        "(\n    base: {},\n    component: {},\n    divider: {},\n    on: {},\n    small_widget: {},\n)",
        ron_color(base),
        indent(component_body),
        ron_color(divider),
        ron_color(on),
        ron_color(base.with_alpha(0.25)),
    )
}

/// Indent a nested RON block by four spaces so the file stays readable.
fn indent(block: &str) -> String {
    block
        .lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Mix two colours by `t` in the direction of `to`.
fn mix(from: Rgba, to: Rgba, t: f32) -> Rgba {
    from.mix(to, t)
}

/// A neutral ramp from `dark` to `light`, sampled at `t`.
///
/// COSMIC's palette wants eleven `neutral_0..neutral_10` steps; deriving them
/// from the theme's own background and text keeps the ramp consistent with
/// the rest of the palette instead of shipping an unrelated grey scale.
fn neutral(dark: Rgba, light: Rgba, t: f32) -> Rgba {
    Rgba {
        a: 1.0,
        ..mix(dark, light, t)
    }
}

impl DesignSystem {
    /// The COSMIC config id for this appearance.
    #[must_use]
    pub fn cosmic_theme_id(&self) -> &'static str {
        if self.is_dark {
            DARK_THEME_ID
        } else {
            LIGHT_THEME_ID
        }
    }

    /// Build COSMIC's 32-entry palette from this design system.
    ///
    /// `CosmicPaletteInner` is `name` plus 32 colours. Inventing them would
    /// produce a desktop that clashes with its own chrome, so they are
    /// derived: neutrals from the theme's background-to-text axis, the
    /// accent family by rotating the accent's hue, and the extended colours
    /// from fixed, conventional hues at a saturation the theme can carry.
    #[must_use]
    pub fn cosmic_palette_body(&self) -> String {
        let bg = self.window_background;
        let fg = self.text;
        let dark = if self.is_dark { bg } else { fg };
        let light = if self.is_dark { fg } else { bg };
        let mut out = String::new();
        let _ = writeln!(out, "(");
        let _ = writeln!(
            out,
            "    name: {:?},",
            if self.is_dark {
                "tahoe-dark"
            } else {
                "tahoe-light"
            }
        );
        // The accent family: COSMIC reserves eight of them, and a theme that
        // leaves them at upstream defaults looks borrowed.
        let a = self.accent;
        let brighten = |tint: Rgba, amount: f32| Rgba {
            a: 1.0,
            ..tint.mix(Rgba::new(1.0, 1.0, 1.0, 1.0), amount)
        };
        let _ = writeln!(
            out,
            "    bright_red: {},",
            ron_color(brighten(hue_shift(a, 0.0), 0.25))
        );
        let _ = writeln!(
            out,
            "    bright_green: {},",
            ron_color(brighten(hue_shift(a, 0.33), 0.25))
        );
        let _ = writeln!(
            out,
            "    bright_orange: {},",
            ron_color(brighten(hue_shift(a, 0.08), 0.25))
        );
        let _ = writeln!(
            out,
            "    gray_1: {},",
            ron_color(neutral(dark, light, 0.10))
        );
        let _ = writeln!(
            out,
            "    gray_2: {},",
            ron_color(neutral(dark, light, 0.20))
        );
        for step in 0..=10u32 {
            let t = step as f32 / 10.0;
            let _ = writeln!(
                out,
                "    neutral_{step}: {},",
                ron_color(neutral(dark, light, t))
            );
        }
        for (name, shift) in [
            ("accent_blue", 0.58),
            ("accent_indigo", 0.66),
            ("accent_purple", 0.75),
            ("accent_pink", 0.92),
            ("accent_red", 0.0),
            ("accent_orange", 0.08),
            ("accent_yellow", 0.14),
            ("accent_green", 0.33),
        ] {
            let _ = writeln!(out, "    {name}: {},", ron_color(hue_shift(a, shift)));
        }
        let _ = writeln!(
            out,
            "    accent_warm_grey: {},",
            ron_color(neutral(dark, light, 0.55))
        );
        for (name, shift, amount) in [
            ("ext_warm_grey", 0.05, 0.75),
            ("ext_orange", 0.07, 0.55),
            ("ext_yellow", 0.13, 0.50),
            ("ext_blue", 0.56, 0.55),
            ("ext_purple", 0.76, 0.55),
            ("ext_pink", 0.93, 0.55),
            ("ext_indigo", 0.67, 0.55),
        ] {
            let base = if amount >= 0.7 {
                neutral(dark, light, 0.55)
            } else {
                a
            };
            let tinted = if amount >= 0.7 {
                base
            } else {
                hue_shift(base, shift)
            };
            let _ = writeln!(out, "    {name}: {},", ron_color(Rgba { a: 1.0, ..tinted }));
        }
        let _ = writeln!(out, ")");
        out
    }

    /// Every file COSMIC needs for this appearance.
    #[must_use]
    pub fn to_cosmic_theme(&self) -> Vec<ThemeFile> {
        let base = format!("{}/v{THEME_VERSION}", self.cosmic_theme_id());
        let bg = self.window_background;
        let panel = self.panel;
        let fg = self.text;
        let muted = self.text_muted;
        let accent = self.accent;
        let divider = self.separator;

        // Surface levels. COSMIC's `primary` is the main surface and
        // `secondary` sits on top of it; Tahoe's panel and dock are exactly
        // those two, so they map without invention.
        let primary_base = bg;
        let secondary_base = mix(bg, panel, 0.5);
        let component_on = fg;
        let component_divider = divider;
        let hover = mix(bg, fg, 0.10);
        let pressed = mix(bg, fg, 0.18);
        let border = mix(bg, fg, 0.30);

        let button_component = component(
            mix(bg, fg, 0.14),
            hover.with_alpha(0.40),
            pressed.with_alpha(0.62),
            component_on,
            component_divider,
            accent,
            border,
        );

        let mut files = vec![
            ThemeFile::new(format!("{base}/name"), format!("{:?}", self.cosmic_name())),
            ThemeFile::new(format!("{base}/is_dark"), self.is_dark.to_string()),
            ThemeFile::new(
                format!("{base}/background"),
                container(
                    primary_base,
                    component_on,
                    component_divider,
                    &component(
                        secondary_base,
                        hover,
                        pressed,
                        component_on,
                        component_divider,
                        accent,
                        border,
                    ),
                ),
            ),
            ThemeFile::new(
                format!("{base}/primary"),
                container(
                    primary_base,
                    component_on,
                    component_divider,
                    &component(
                        mix(primary_base, fg, 0.06),
                        hover,
                        pressed,
                        component_on,
                        component_divider,
                        accent,
                        border,
                    ),
                ),
            ),
            ThemeFile::new(
                format!("{base}/secondary"),
                container(
                    secondary_base,
                    component_on,
                    component_divider,
                    &component(
                        mix(secondary_base, fg, 0.08),
                        hover,
                        pressed,
                        component_on,
                        component_divider,
                        accent,
                        border,
                    ),
                ),
            ),
            ThemeFile::new(
                format!("{base}/transparent_background"),
                container(
                    primary_base.with_alpha(0.0),
                    component_on,
                    component_divider,
                    &component(
                        secondary_base.with_alpha(0.0),
                        hover,
                        pressed,
                        component_on,
                        component_divider,
                        accent,
                        border,
                    ),
                ),
            ),
            ThemeFile::new(
                format!("{base}/transparent_primary"),
                container(
                    primary_base.with_alpha(0.0),
                    component_on,
                    component_divider,
                    &component(
                        primary_base.with_alpha(0.0),
                        hover,
                        pressed,
                        component_on,
                        component_divider,
                        accent,
                        border,
                    ),
                ),
            ),
            ThemeFile::new(
                format!("{base}/transparent_secondary"),
                container(
                    secondary_base.with_alpha(0.0),
                    component_on,
                    component_divider,
                    &component(
                        secondary_base.with_alpha(0.0),
                        hover,
                        pressed,
                        component_on,
                        component_divider,
                        accent,
                        border,
                    ),
                ),
            ),
            ThemeFile::new(format!("{base}/button"), button_component.clone()),
            ThemeFile::new(format!("{base}/icon_button"), button_component.clone()),
            ThemeFile::new(format!("{base}/list_button"), button_component.clone()),
            ThemeFile::new(format!("{base}/text_button"), button_component.clone()),
            ThemeFile::new(format!("{base}/accent"), ron_color(accent.with_alpha(0.30))),
            ThemeFile::new(format!("{base}/accent_text"), ron_color(accent)),
            ThemeFile::new(
                format!("{base}/success"),
                ron_color(hue_shift(accent, 0.33)),
            ),
            ThemeFile::new(
                format!("{base}/destructive"),
                ron_color(hue_shift(accent, 0.0)),
            ),
            ThemeFile::new(
                format!("{base}/warning"),
                ron_color(hue_shift(accent, 0.10)),
            ),
            ThemeFile::new(format!("{base}/success_button"), button_component.clone()),
            ThemeFile::new(
                format!("{base}/destructive_button"),
                button_component.clone(),
            ),
            ThemeFile::new(format!("{base}/warning_button"), button_component.clone()),
            ThemeFile::new(format!("{base}/accent_button"), button_component.clone()),
            ThemeFile::new(format!("{base}/link_button"), button_component),
            ThemeFile::new(format!("{base}/palette"), self.cosmic_palette_body()),
            ThemeFile::new(
                format!("{base}/corner_radii"),
                format!(
                    "(\n    radius_0: {},\n    radius_xs: {},\n    radius_s: {},\n    radius_m: {},\n    radius_l: {},\n    radius_xl: {},\n)",
                    ron_quad(0.0),
                    ron_quad(self.radii.control as f32),
                    ron_quad(self.radii.card as f32),
                    ron_quad(self.radii.window as f32),
                    ron_quad(self.radii.dock as f32),
                    // Upstream's XL is 160; keeping it means a pill-shaped
                    // panel keeps working.
                    ron_quad(160.0),
                ),
            ),
            ThemeFile::new(
                format!("{base}/spacing"),
                format!(
                    "(\n    space_none: 0,\n    space_xxxs: {},\n    space_xxs: {},\n    space_xs: {},\n    space_s: {},\n    space_m: {},\n    space_l: {},\n    space_xl: {},\n    space_xxl: {},\n    space_xxxl: {},\n)",
                    // COSMIC's nine steps, with the design system's five
                    // mapped onto the first five and the rest continuing the
                    // scale rather than stopping. A ladder that plateaus
                    // makes distant steps look identical.
                    self.spacing.xs,
                    self.spacing.sm,
                    self.spacing.md,
                    self.spacing.lg,
                    self.spacing.xl,
                    self.spacing.xl + self.spacing.md,
                    self.spacing.xl * 2,
                    self.spacing.xl * 3,
                    self.spacing.xl * 4,
                ),
            ),
            // Window gaps and the focus hint are theme fields, which is worth
            // demonstrating: they are restylable, unlike the animations.
            ThemeFile::new(format!("{base}/gaps"), format!("(0, {})", self.spacing.sm)),
            ThemeFile::new(format!("{base}/active_hint"), "3".to_owned()),
            ThemeFile::new(
                format!("{base}/window_hint"),
                ron_optional_color(Some(accent.with_alpha(0.55))),
            ),
            // Tahoe's chrome is translucent by design, so the frosted flags
            // follow from the design system rather than being turned on
            // arbitrarily.
            ThemeFile::new(format!("{base}/frosted"), "Medium".to_owned()),
            ThemeFile::new(format!("{base}/frosted_windows"), "false".to_owned()),
            ThemeFile::new(
                format!("{base}/frosted_system_interface"),
                (panel.a < 1.0).to_string(),
            ),
            ThemeFile::new(format!("{base}/frosted_panel"), (panel.a < 1.0).to_string()),
            ThemeFile::new(format!("{base}/frosted_applets"), "false".to_owned()),
            // The 39th field. Upstream's shipped schema omits it, so a
            // reader copying that file would miss it; the struct has it and
            // leaving it out silently keeps the compiled default.
            ThemeFile::new(format!("{base}/frosted_maximized_apps"), "false".to_owned()),
            // `shade` is an option upstream; a bare colour there would lose
            // the `Some` and fail to parse.
            ThemeFile::new(
                format!("{base}/shade"),
                ron_optional_color(Some(Rgba {
                    a: 1.0,
                    ..mix(bg, Rgba::new(0.0, 0.0, 0.0, 1.0), 0.35)
                })),
            ),
            ThemeFile::new(format!("{base}/control_tint"), ron_color(fg)),
            ThemeFile::new(format!("{base}/text_tint"), ron_color(fg)),
            ThemeFile::new(format!("{base}/alpha_map"), alpha_map(fg)),
            ThemeFile::new(format!("{base}/is_high_contrast"), "false".to_owned()),
        ];
        // Only emit a secondary text colour where COSMIC has a place for it.
        let _ = muted;
        files.sort_by(|a, b| a.path.cmp(&b.path));
        files
    }

    fn cosmic_name(&self) -> String {
        format!("tahoe-{}", if self.is_dark { "dark" } else { "light" })
    }

    /// The theme-mode file, which decides dark or light at login.
    #[must_use]
    pub fn cosmic_mode_files(&self) -> Vec<ThemeFile> {
        vec![
            ThemeFile::new(
                format!("{THEME_MODE_ID}/v{MODE_VERSION}/is_dark"),
                self.is_dark.to_string(),
            ),
            ThemeFile::new(
                format!("{THEME_MODE_ID}/v{MODE_VERSION}/auto_switch"),
                "false".to_owned(),
            ),
        ]
    }

    /// A panel and dock layout, as system defaults.
    ///
    /// The key set is fixed: panel structs set `deny_unknown_fields`, so a
    /// misspelled key aborts the whole config load. The tests check every
    /// emitted key against the verified upstream list.
    #[must_use]
    pub fn cosmic_panel_files(&self) -> Vec<ThemeFile> {
        let panel = |profile: &str, key: &str, value: &str| {
            ThemeFile::new(
                format!("{PANEL_ID}.{profile}/v{PANEL_VERSION}/{key}"),
                value.to_owned(),
            )
        };
        vec![
            ThemeFile::new(
                format!("{PANEL_ID}/v{PANEL_VERSION}/entries"),
                r#"["Panel", "Dock"]"#.to_owned(),
            ),
            // A top panel with the launcher and the clock, matching the strip
            // the compositor reserves.
            panel("Panel", "name", r#""Panel""#),
            panel("Panel", "anchor", "Top"),
            panel("Panel", "size", "M"),
            panel("Panel", "expand_to_edges", "true"),
            panel("Panel", "border_radius", "0"),
            panel("Panel", "exclusive_zone", "true"),
            panel("Panel", "opacity", "1.0"),
            panel("Panel", "background", "ThemeDefault"),
            panel("Panel", "autohide", "Never"),
            // A bottom dock, sized and rounded from the design system.
            panel("Dock", "name", r#""Dock""#),
            panel("Dock", "anchor", "Bottom"),
            panel("Dock", "size", "L"),
            panel("Dock", "expand_to_edges", "false"),
            panel("Dock", "border_radius", &self.radii.dock.to_string()),
            panel("Dock", "exclusive_zone", "true"),
            panel("Dock", "padding", &self.spacing.xs.to_string()),
            panel("Dock", "spacing", &self.spacing.sm.to_string()),
            panel("Dock", "opacity", &cosmic_float(self.dock.a)),
            panel("Dock", "background", "ThemeDefault"),
            panel("Dock", "autohide", "Never"),
        ]
    }

    /// Everything a distribution installs under `/usr/share/cosmic`.
    #[must_use]
    pub fn to_cosmic_tree(&self, other: &Self) -> Vec<ThemeFile> {
        let mut files = self.to_cosmic_theme();
        files.extend(other.to_cosmic_theme());
        files.extend(self.cosmic_mode_files());
        files.extend(self.cosmic_panel_files());
        files.sort_by(|a, b| a.path.cmp(&b.path));
        files.dedup_by(|a, b| a.path == b.path);
        files
    }
}

/// Format an `f32` for RON: RON wants a decimal point for floats.
#[must_use]
pub fn cosmic_float(v: f32) -> String {
    super::swift::swift_float(v)
}

/// The `alpha_map`: COSMIC's per-step opacity ladder.
fn alpha_map(fg: Rgba) -> String {
    let steps = [
        0.0f32, 0.05, 0.10, 0.15, 0.20, 0.25, 0.30, 0.40, 0.50, 0.65, 0.80, 1.0,
    ];
    let body = steps
        .iter()
        .map(|a| ron_color(fg.with_alpha(*a)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("({body})")
}

/// Rotate a colour's hue by `amount` turns, preserving lightness.
///
/// A minimal RGB-space rotation is enough here: the palette wants recognisably
/// different hues at a consistent lightness, not colorimetric accuracy, and a
/// full colour-space conversion would be a dependency for a cosmetic result.
fn hue_shift(c: Rgba, amount: f32) -> Rgba {
    // Convert to a coarse HSV, rotate, convert back.
    let max = c.r.max(c.g).max(c.b);
    let min = c.r.min(c.g).min(c.b);
    let delta = max - min;
    let mut h = if delta <= f32::EPSILON {
        0.0
    } else if (max - c.r).abs() <= f32::EPSILON {
        60.0 * (((c.g - c.b) / delta) % 6.0)
    } else if (max - c.g).abs() <= f32::EPSILON {
        60.0 * ((c.b - c.r) / delta + 2.0)
    } else {
        60.0 * ((c.r - c.g) / delta + 4.0)
    };
    if h < 0.0 {
        h += 360.0;
    }
    h = (h + amount * 360.0) % 360.0;

    let s = if max <= f32::EPSILON {
        0.0
    } else {
        delta / max
    };
    let v = max;
    let c2 = v * s;
    let x = c2 * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c2;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c2, x, 0.0),
        1 => (x, c2, 0.0),
        2 => (0.0, c2, x),
        3 => (0.0, x, c2),
        4 => (x, 0.0, c2),
        _ => (c2, 0.0, x),
    };
    Rgba::new(r + m, g + m, b + m, c.a)
}

#[cfg(test)]
mod tests;
