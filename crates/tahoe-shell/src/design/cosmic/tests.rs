// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the COSMIC theme emitter.
//!
//! The point of these is that a wrong RON key or a wrong colour form is a
//! silent failure in COSMIC — theme structs ignore unknown fields and fall
//! back to the compiled defaults, so a typo produces a desktop that looks
//! *nearly* right. Panel structs do the opposite and refuse to load. Both
//! failure modes are pinned here.

use super::*;
use crate::design::DesignSystem;

fn dark() -> DesignSystem {
    DesignSystem::dark()
}

fn tree() -> Vec<ThemeFile> {
    dark().to_cosmic_tree(&DesignSystem::light())
}

fn find<'a>(files: &'a [ThemeFile], suffix: &str) -> &'a ThemeFile {
    files
        .iter()
        .find(|f| f.path.ends_with(suffix))
        .unwrap_or_else(|| {
            panic!(
                "no file ending in {suffix}; have {:?}",
                files.iter().map(|f| &f.path).collect::<Vec<_>>()
            )
        })
}

// ---------------------------------------------------------------------------
// Colour and number forms
// ---------------------------------------------------------------------------

#[test]
fn colours_are_eight_hex_digits_not_tuples() {
    // `cosmic_theme::color_serde` writes `#RRGGBBAA`. A tuple would parse,
    // because the deserialiser accepts both, but it would not match what
    // upstream's own schema files contain and would look wrong to a reader.
    assert_eq!(
        ron_color(crate::design::Rgba::new(1.0, 0.0, 0.0, 1.0)),
        "\"#FF0000FF\""
    );
    assert_eq!(
        ron_color(crate::design::Rgba::new(0.0, 0.0, 0.0, 0.5)),
        "\"#00000080\""
    );
    assert_eq!(
        ron_color(crate::design::Rgba::new(0.0, 0.0, 1.0, 0.0)),
        "\"#0000FF00\""
    );
    // Quoted, because upstream's schema writes `base: "#9E9E9E40"` and a
    // bare `#…` is not a RON value at all.
    assert!(ron_color(dark().accent).starts_with("\"#"));
    assert_eq!(ron_color(dark().accent).len(), 11);
}

#[test]
fn an_opaque_colour_ends_in_ff() {
    // `Srgb` becomes `Srgba` with alpha FF; getting this wrong makes every
    // surface transparent. The separator is deliberately translucent, so it
    // is not in this list.
    for c in [dark().text, dark().window_background, dark().accent] {
        assert!(ron_color(c).ends_with("FF\""), "{c:?} should be opaque");
    }
    assert!(
        dark().separator.a < 1.0,
        "the separator is a hairline by design"
    );
}

#[test]
fn optional_colours_carry_their_some() {
    // `window_hint` and `shade` are `Option<Srgb>`; a bare colour there
    // loses the `Some` and fails to parse.
    assert_eq!(ron_optional_color(None), "None");
    let some = ron_optional_color(Some(crate::design::Rgba::new(1.0, 0.0, 0.0, 1.0)));
    assert_eq!(some, "Some(\"#FF0000FF\")");
    let files = tree();
    assert!(
        find(&files, "CosmicTheme.Dark/v2/window_hint")
            .body
            .starts_with("Some(")
    );
    assert!(
        find(&files, "CosmicTheme.Dark/v2/shade")
            .body
            .starts_with("Some(")
    );
}

#[test]
fn floats_always_carry_a_decimal_point() {
    // RON parses `4` as an integer and `4.0` as a float; the fields are f32.
    assert_eq!(ron_quad(0.0), "(0.0, 0.0, 0.0, 0.0)");
    assert_eq!(ron_quad(12.0), "(12.0, 12.0, 12.0, 12.0)");
    assert_eq!(cosmic_float(1.0), "1.0");
    assert_eq!(cosmic_float(0.72), "0.72");
}

#[test]
fn no_exponent_notation_reaches_the_theme() {
    // Only in numbers: matching a bare `e` would trip on `Never` and
    // `ThemeDefault`, which are legitimate identifiers.
    for f in tree() {
        let bytes = f.body.as_bytes();
        for (i, b) in bytes.iter().enumerate() {
            if *b != b'e' {
                continue;
            }
            let before_digit = i > 0 && bytes[i - 1].is_ascii_digit();
            let after_numeric = bytes
                .get(i + 1)
                .is_some_and(|n| n.is_ascii_digit() || *n == b'+' || *n == b'-');
            assert!(
                !(before_digit && after_numeric),
                "{}: exponent notation in {:?}",
                f.path,
                f.body
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The theme tree
// ---------------------------------------------------------------------------

#[test]
fn the_tree_lives_under_the_path_cosmic_reads() {
    // `find_data_file` on `BaseDirectories::with_prefix("cosmic")` resolves
    // /usr/share/cosmic; a file one directory away is never read.
    for f in tree() {
        assert!(
            f.path.starts_with("com.system76."),
            "{} is not under a config id",
            f.path
        );
    }
    assert_eq!(SYSTEM_PREFIX, "/usr/share/cosmic");
}

#[test]
fn both_appearances_are_emitted_at_the_right_version() {
    let files = tree();
    for id in [DARK_THEME_ID, LIGHT_THEME_ID] {
        let prefix = format!("{id}/v{THEME_VERSION}/name");
        let name = find(&files, &prefix);
        assert!(
            name.body.starts_with('"'),
            "name must be a quoted string: {:?}",
            name.body
        );
    }
    assert_eq!(THEME_VERSION, 2, "cosmic_theme::Theme is #[version = 2]");
}

#[test]
fn every_file_declares_the_darkness_it_belongs_to() {
    let files = tree();
    assert_eq!(find(&files, "CosmicTheme.Dark/v2/is_dark").body, "true");
    assert_eq!(find(&files, "CosmicTheme.Light/v2/is_dark").body, "false");
}

#[test]
fn the_names_are_distinguishable() {
    let files = tree();
    assert_eq!(
        find(&files, "CosmicTheme.Dark/v2/name").body,
        "\"tahoe-dark\""
    );
    assert_eq!(
        find(&files, "CosmicTheme.Light/v2/name").body,
        "\"tahoe-light\""
    );
}

#[test]
fn the_theme_carries_every_field_upstream_has() {
    // The 39 fields of `cosmic_theme::Theme`. A missing one is not fatal —
    // it falls back to the compiled default — which is exactly why it needs
    // asserting rather than hoping.
    let required = [
        "name",
        "background",
        "primary",
        "secondary",
        "transparent_background",
        "transparent_primary",
        "transparent_secondary",
        "button",
        "accent",
        "success",
        "destructive",
        "warning",
        "accent_button",
        "success_button",
        "destructive_button",
        "warning_button",
        "icon_button",
        "link_button",
        "list_button",
        "text_button",
        "palette",
        "spacing",
        "corner_radii",
        "is_dark",
        "is_high_contrast",
        "gaps",
        "active_hint",
        "window_hint",
        "frosted",
        "frosted_windows",
        "frosted_system_interface",
        "frosted_panel",
        "frosted_applets",
        "frosted_maximized_apps",
        "alpha_map",
        "shade",
        "accent_text",
        "control_tint",
        "text_tint",
    ];
    assert_eq!(required.len(), 39, "upstream's Theme has 39 fields");
    let files = tree();
    for field in required {
        let path = format!("{DARK_THEME_ID}/v{THEME_VERSION}/{field}");
        assert!(
            files.iter().any(|f| f.path == path),
            "the dark theme is missing `{field}`"
        );
    }
}

#[test]
fn corner_radii_come_from_the_design_system() {
    // The four own-brand radii, in COSMIC's six-step scale. This is what
    // makes a COSMIC window have a Tahoe corner instead of a COSMIC one.
    let d = dark();
    let files = tree();
    let body = &find(&files, "CosmicTheme.Dark/v2/corner_radii").body;
    assert!(
        body.contains(&format!("radius_xs: {}", ron_quad(d.radii.control as f32))),
        "{body}"
    );
    assert!(
        body.contains(&format!("radius_s: {}", ron_quad(d.radii.card as f32))),
        "{body}"
    );
    assert!(
        body.contains(&format!("radius_m: {}", ron_quad(d.radii.window as f32))),
        "{body}"
    );
    assert!(
        body.contains(&format!("radius_l: {}", ron_quad(d.radii.dock as f32))),
        "{body}"
    );
    // Upstream's XL is a pill; keeping it means a rounded panel still works.
    assert!(body.contains("radius_xl: (160.0"), "{body}");
    assert!(body.contains("radius_0: (0.0, 0.0, 0.0, 0.0)"), "{body}");
}

#[test]
fn spacing_comes_from_the_design_system() {
    let d = dark();
    let files = tree();
    let body = &find(&files, "CosmicTheme.Dark/v2/spacing").body;
    // The design system's five tokens land on COSMIC's first five steps;
    // the remaining four continue the ladder so distant steps stay distinct.
    for (name, value) in [
        ("space_xxxs", d.spacing.xs),
        ("space_xxs", d.spacing.sm),
        ("space_xs", d.spacing.md),
        ("space_s", d.spacing.lg),
        ("space_m", d.spacing.xl),
        ("space_l", d.spacing.xl + d.spacing.md),
        ("space_xl", d.spacing.xl * 2),
        ("space_xxl", d.spacing.xl * 3),
        ("space_xxxl", d.spacing.xl * 4),
    ] {
        assert!(
            body.contains(&format!("{name}: {value}")),
            "{name} missing from {body}"
        );
    }
    // COSMIC's own spacing is u16, so no decimal point belongs here.
    assert!(!body.contains('.'), "spacing must be integers: {body}");
}

#[test]
fn the_accent_is_the_design_systems_accent() {
    let d = dark();
    let files = tree();
    let body = &find(&files, "CosmicTheme.Dark/v2/accent_text").body;
    assert_eq!(*body, ron_color(d.accent));
}

#[test]
fn frosted_follows_from_the_translucent_chrome() {
    // Tahoe's panel is a translucent material by design; COSMIC should blur
    // what shows through rather than leaving it sharp.
    let d = dark();
    assert!(d.panel.a < 1.0, "the test assumes a translucent panel");
    let files = tree();
    assert_eq!(
        find(&files, "CosmicTheme.Dark/v2/frosted_panel").body,
        "true"
    );
    assert_eq!(
        find(&files, "CosmicTheme.Dark/v2/frosted_windows").body,
        "false"
    );
}

#[test]
fn the_palette_has_all_thirty_two_colours_plus_a_name() {
    let files = tree();
    let body = &find(&files, "CosmicTheme.Dark/v2/palette").body;
    assert!(body.contains("name: \"tahoe-dark\""), "{body}");
    for step in 0..=10 {
        assert!(
            body.contains(&format!("neutral_{step}:")),
            "neutral_{step} missing"
        );
    }
    for name in [
        "bright_red",
        "bright_green",
        "bright_orange",
        "gray_1",
        "gray_2",
        "accent_blue",
        "accent_indigo",
        "accent_purple",
        "accent_pink",
        "accent_red",
        "accent_orange",
        "accent_yellow",
        "accent_green",
        "accent_warm_grey",
        "ext_warm_grey",
        "ext_orange",
        "ext_yellow",
        "ext_blue",
        "ext_purple",
        "ext_pink",
        "ext_indigo",
    ] {
        assert!(
            body.contains(&format!("{name}:")),
            "{name} missing from the palette"
        );
    }
}

#[test]
fn the_neutral_ramp_is_ordered() {
    // A ramp that is not monotonic makes the palette look broken wherever
    // COSMIC picks adjacent steps.
    let files = tree();
    let body = &find(&files, "CosmicTheme.Dark/v2/palette").body;
    let mut previous = -1.0f32;
    for step in 0..=10 {
        // Find the key, then read the first hex byte after the opening quote.
        let key = format!("neutral_{step}: ");
        let at = body
            .find(&key)
            .unwrap_or_else(|| panic!("neutral_{step} missing"));
        let rest = &body[at + key.len()..];
        let open = rest
            .find('"')
            .unwrap_or_else(|| panic!("neutral_{step} is not a quoted string"));
        // Skip the quote and the `#`, so this reads the red byte.
        assert_eq!(
            &rest[open + 1..open + 2],
            "#",
            "neutral_{step} is not #RRGGBBAA"
        );
        let hex = &rest[open + 2..open + 4];
        let r = u8::from_str_radix(hex, 16).expect("two hex digits");
        assert!(
            f32::from(r) >= previous,
            "neutral_{step} is darker than the step below it"
        );
        previous = f32::from(r);
    }
}

#[test]
fn the_dark_and_light_palettes_differ() {
    let files = tree();
    let dark = &find(&files, "CosmicTheme.Dark/v2/palette").body;
    let light = &find(&files, "CosmicTheme.Light/v2/palette").body;
    assert_ne!(dark, light);
    assert!(light.contains("name: \"tahoe-light\""));
}

#[test]
fn every_file_is_balanced_and_non_empty() {
    for f in tree() {
        assert!(!f.body.trim().is_empty(), "{} is empty", f.path);
        let mut depth = 0i32;
        for c in f.body.chars() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            assert!(depth >= 0, "{} closes before it opens", f.path);
        }
        assert_eq!(
            depth, 0,
            "{} has unbalanced parentheses: {}",
            f.path, f.body
        );
    }
}

#[test]
fn file_paths_are_unique() {
    let files = tree();
    let mut seen = std::collections::BTreeSet::new();
    for f in &files {
        assert!(seen.insert(f.path.clone()), "duplicate path {}", f.path);
    }
}

#[test]
fn generation_is_deterministic() {
    assert_eq!(tree(), tree());
}

// ---------------------------------------------------------------------------
// Panel
// ---------------------------------------------------------------------------

/// The exact key set from `cosmic-panel-config::CosmicPanelConfig`.
///
/// Panel structs set `deny_unknown_fields`, so a key outside this list makes
/// the whole panel refuse to load — the opposite failure from the theme,
/// where a typo is silent. Both are worth pinning.
const PANEL_KEYS: &[&str] = &[
    "name",
    "anchor",
    "anchor_gap",
    "layer",
    "keyboard_interactivity",
    "size",
    "output",
    "background",
    "plugins_wings",
    "plugins_center",
    "size_wings",
    "size_center",
    "expand_to_edges",
    "padding",
    "spacing",
    "border_radius",
    "border_width",
    "exclusive_zone",
    "autohide",
    "autohide_behavior",
    "margin",
    "opacity",
    "autohover_delay_ms",
    "padding_overlap",
    "keep_style_on_maximize",
];

#[test]
fn panel_keys_are_all_real_upstream_keys() {
    let files = dark().cosmic_panel_files();
    for f in &files {
        // com.system76.CosmicPanel.Panel/v1/<key>
        let Some((_, key)) = f.path.rsplit_once('/') else {
            continue;
        };
        if !f.path.contains("CosmicPanel.") {
            continue;
        }
        assert!(
            PANEL_KEYS.contains(&key),
            "`{key}` is not a CosmicPanelConfig field; with deny_unknown_fields \
             this would make the panel refuse to load"
        );
    }
}

#[test]
fn the_panel_registry_lists_the_profiles_that_exist() {
    let files = dark().cosmic_panel_files();
    let entries = find(&files, "CosmicPanel/v1/entries");
    assert_eq!(entries.body, r#"["Panel", "Dock"]"#.to_owned());
    // Every profile named there must have files, or COSMIC looks for a panel
    // that does not exist.
    for profile in ["Panel", "Dock"] {
        assert!(
            files
                .iter()
                .any(|f| f.path.contains(&format!("CosmicPanel.{profile}/"))),
            "no config for profile {profile}"
        );
    }
}

#[test]
fn the_dock_is_a_bottom_pill_sized_from_the_design_system() {
    let d = dark();
    let files = d.cosmic_panel_files();
    assert_eq!(find(&files, "CosmicPanel.Dock/v1/anchor").body, "Bottom");
    assert_eq!(
        find(&files, "CosmicPanel.Dock/v1/border_radius").body,
        d.radii.dock.to_string()
    );
    assert_eq!(
        find(&files, "CosmicPanel.Dock/v1/opacity").body,
        cosmic_float(d.dock.a)
    );
    assert_eq!(
        find(&files, "CosmicPanel.Dock/v1/exclusive_zone").body,
        "true"
    );
}

#[test]
fn the_top_panel_spans_the_edges() {
    // The compositor reserves a full-width strip; a panel that did not
    // expand to the edges would leave a gap at both ends of it.
    let files = dark().cosmic_panel_files();
    assert_eq!(find(&files, "CosmicPanel.Panel/v1/anchor").body, "Top");
    assert_eq!(
        find(&files, "CosmicPanel.Panel/v1/expand_to_edges").body,
        "true"
    );
    assert_eq!(find(&files, "CosmicPanel.Panel/v1/border_radius").body, "0");
}

#[test]
fn panel_enum_values_are_bare_identifiers() {
    // RON writes unit enum variants without quotes: `Bottom`, not `"Bottom"`.
    let files = dark().cosmic_panel_files();
    for (suffix, expected) in [
        ("Dock/v1/anchor", "Bottom"),
        ("Dock/v1/size", "L"),
        ("Dock/v1/background", "ThemeDefault"),
        ("Dock/v1/autohide", "Never"),
        ("Panel/v1/anchor", "Top"),
        ("Panel/v1/size", "M"),
    ] {
        let body = &find(&files, suffix).body;
        assert_eq!(body, expected, "{suffix}");
        assert!(!body.starts_with('"'), "{suffix} must not be quoted");
    }
}

#[test]
fn panel_booleans_and_floats_are_the_right_type() {
    let files = dark().cosmic_panel_files();
    assert_eq!(find(&files, "Panel/v1/exclusive_zone").body, "true");
    assert_eq!(find(&files, "Dock/v1/expand_to_edges").body, "false");
    // opacity is f32: `1` would be an integer and would not parse.
    assert!(find(&files, "Panel/v1/opacity").body.contains('.'));
}

// ---------------------------------------------------------------------------
// Mode
// ---------------------------------------------------------------------------

#[test]
fn the_mode_file_decides_the_appearance() {
    let files = dark().cosmic_mode_files();
    assert_eq!(find(&files, "CosmicTheme.Mode/v1/is_dark").body, "true");
    assert_eq!(
        find(&files, "CosmicTheme.Mode/v1/auto_switch").body,
        "false"
    );
    let light = DesignSystem::light().cosmic_mode_files();
    assert_eq!(find(&light, "CosmicTheme.Mode/v1/is_dark").body, "false");
}

// ---------------------------------------------------------------------------
// Hue shifting, which the palette depends on
// ---------------------------------------------------------------------------

#[test]
fn hue_shift_preserves_alpha_and_lightness() {
    let c = crate::design::Rgba::new(0.5, 0.2, 0.8, 0.75);
    let shifted = hue_shift(c, 0.5);
    assert!((shifted.a - c.a).abs() < 1e-6, "alpha must survive");
    let lum = |x: crate::design::Rgba| x.r + x.g + x.b;
    assert!(
        (lum(shifted) - lum(c)).abs() < 0.05,
        "lightness drifted: {c:?} -> {shifted:?}"
    );
}

#[test]
fn hue_shift_actually_changes_the_hue() {
    let red = crate::design::Rgba::new(1.0, 0.0, 0.0, 1.0);
    let green = hue_shift(red, 0.33);
    assert!(
        green.g > green.r,
        "a third of a turn from red should be greenish: {green:?}"
    );
    let blue = hue_shift(red, 0.66);
    assert!(blue.b > blue.r, "two thirds should be blueish: {blue:?}");
}

#[test]
fn a_grey_has_no_hue_to_shift() {
    // Zero saturation must not produce NaN through the zero-width branch.
    let grey = crate::design::Rgba::new(0.5, 0.5, 0.5, 1.0);
    let shifted = hue_shift(grey, 0.3);
    assert!(shifted.r.is_finite() && shifted.g.is_finite() && shifted.b.is_finite());
    assert!((shifted.r - 0.5).abs() < 1e-5, "{shifted:?}");
}

#[test]
fn shifted_colours_stay_in_range() {
    for amount in [0.0f32, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0] {
        for base in [
            crate::design::Rgba::new(1.0, 0.0, 0.0, 1.0),
            crate::design::Rgba::new(0.0, 0.0, 0.0, 1.0),
            crate::design::Rgba::new(0.2, 0.9, 0.4, 1.0),
        ] {
            let s = hue_shift(base, amount);
            for component in [s.r, s.g, s.b] {
                assert!(
                    (0.0..=1.0).contains(&component),
                    "{base:?} shifted by {amount} produced {component}"
                );
            }
        }
    }
}
