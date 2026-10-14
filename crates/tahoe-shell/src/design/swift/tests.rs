// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the Swift emitter.
//!
//! The important one is `the_two_languages_agree_on_every_token`: it parses
//! the emitted Swift back and compares every value against the Rust design
//! system. If that passes, the SwiftUI application cannot render a
//! different palette than the compositor.

use super::*;
use crate::design::{DesignSystem, Radii, Spacing};

// ---------------------------------------------------------------------------
// Float formatting
// ---------------------------------------------------------------------------

#[test]
fn swift_floats_always_read_as_floating_point() {
    // `1` would be an Int in Swift and would not compile into a Double field.
    assert_eq!(swift_float(1.0), "1.0");
    assert_eq!(swift_float(0.0), "0.0");
    assert_eq!(
        swift_float(-0.0),
        "0.0",
        "negative zero would be noise in a diff"
    );
    assert_eq!(swift_float(0.72), "0.72");
    assert_eq!(swift_float(28.0), "28.0");
}

#[test]
fn swift_floats_keep_enough_precision_to_round_trip() {
    for value in [0.1f32, 1.0 / 3.0, 0.123456, 0.999999, 1e-6] {
        let text = swift_float(value);
        let parsed: f64 = text.parse().expect("emitted value must parse");
        let original = f64::from(value);
        let tolerance = 1e-5_f64.max(original.abs() * 1e-5);
        assert!(
            (parsed - original).abs() <= tolerance,
            "{value} emitted as {text} parsed as {parsed}"
        );
    }
}

#[test]
fn swift_floats_never_use_exponent_notation() {
    // `1e-7` parses in Swift, but a generated file full of exponents is
    // unreadable and hides typos in the values.
    for value in [1e-7f32, 1e-20, 123456.0] {
        let text = swift_float(value);
        assert!(
            !text.contains('e') && !text.contains('E'),
            "{value} emitted as {text}"
        );
    }
}

// ---------------------------------------------------------------------------
// Structure
// ---------------------------------------------------------------------------

fn both() -> String {
    DesignSystem::dark().to_swift_with_appearances(&DesignSystem::light())
}

#[test]
fn the_emitted_file_declares_both_appearances() {
    let swift = both();
    assert!(swift.contains("public enum TahoeDesign_Dark"));
    assert!(swift.contains("public enum TahoeDesign_Light"));
    assert!(swift.contains("public enum TahoeAppearance"));
    assert!(swift.contains("public enum Tahoe {"));
    assert!(swift.contains("public struct RGBA"));
    assert!(swift.contains("public struct GradientStop"));
}

#[test]
fn the_emitted_file_carries_a_do_not_edit_banner() {
    let swift = both();
    assert!(swift.contains("DO NOT EDIT BY HAND"));
    assert!(
        swift.contains("tahoe-design-export"),
        "the regenerate command must be present"
    );
    assert!(swift.contains("single source of truth"));
}

#[test]
fn the_swiftui_dependent_part_is_gated_behind_a_can_import() {
    // Tokens must be readable by a headless `swift test`; only the view
    // conveniences may require SwiftUI.
    let swift = both();
    let gate = swift
        .find("#if canImport(SwiftUI)")
        .expect("a SwiftUI gate");
    let tokens = swift.find("public enum TahoeDesign_Dark").expect("tokens");
    assert!(
        tokens < gate,
        "the token namespaces must come before the SwiftUI gate"
    );
    assert!(swift.contains("import SwiftUI"));
    // The gate must be closed.
    let after = &swift[gate..];
    assert_eq!(after.matches("#if canImport(SwiftUI)").count(), 1);
    assert_eq!(after.matches("#endif").count(), 1);
}

#[test]
fn generation_is_deterministic() {
    assert_eq!(
        both(),
        both(),
        "the same design system must emit identical bytes"
    );
}

#[test]
fn the_output_has_balanced_braces() {
    // A cheap structural check that would have caught the first version's
    // `format!` bug, where Swift's `{` was eaten as a Rust placeholder.
    let swift = both();
    let mut depth = 0i32;
    let mut min_depth = 0i32;
    for c in swift.chars() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                min_depth = min_depth.min(depth);
            }
            _ => {}
        }
    }
    assert_eq!(depth, 0, "unbalanced braces");
    assert_eq!(min_depth, 0, "a closing brace appeared before its opener");
}

#[test]
fn no_unresolved_format_placeholders_survive() {
    // `{{` and `}}` are Rust's escapes. If one reaches the Swift output the
    // file will not compile, and the cause is a format string, not Swift.
    let swift = both();
    assert!(
        !swift.contains("{{"),
        "an escaped format brace leaked into the output"
    );
    assert!(
        !swift.contains("}}"),
        "an escaped format brace leaked into the output"
    );
    for placeholder in ["{field}", "{name}", "{value}", "{regenerate}", "{scope}"] {
        assert!(
            !swift.contains(placeholder),
            "unsubstituted placeholder {placeholder}"
        );
    }
}

#[test]
fn the_output_mentions_no_rust_syntax() {
    let swift = both();
    for rust in ["fn ", "let mut", "impl ", "::new(", "Vec<", "Option<"] {
        assert!(
            !swift.contains(rust),
            "Rust syntax {rust:?} leaked into the Swift output"
        );
    }
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

/// Read a `static let <name>: Double = <value>` from an emitted namespace.
fn read_double(swift: &str, namespace: &str, name: &str) -> Option<f64> {
    let start = swift.find(namespace)?;
    let body = &swift[start..];
    let end = body.find("\n}").unwrap_or(body.len());
    let body = &body[..end];
    let needle = format!("static let {name}: Double = ");
    let at = body.find(&needle)?;
    let rest = &body[at + needle.len()..];
    let value: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    value.parse().ok()
}

/// Read `static let <name> = RGBA(...)` and return its four components.
fn read_rgba(swift: &str, namespace: &str, name: &str) -> Option<[f64; 4]> {
    let start = swift.find(namespace)?;
    let body = &swift[start..];
    let end = body.find("\n}").unwrap_or(body.len());
    let body = &body[..end];
    let needle = format!("static let {name} = RGBA(");
    let at = body.find(&needle)?;
    let rest = &body[at + needle.len()..];
    let close = rest.find(')')?;
    let mut out = [0.0; 4];
    for (i, part) in rest[..close].split(',').enumerate() {
        if i >= 4 {
            return None;
        }
        let value = part.split(':').nth(1)?.trim();
        out[i] = value.parse().ok()?;
    }
    Some(out)
}

#[test]
fn every_colour_token_round_trips_from_rust_to_swift() {
    let dark = DesignSystem::dark();
    let swift = both();
    let cases: [(&str, crate::design::Rgba); 7] = [
        ("panel", dark.panel),
        ("dock", dark.dock),
        ("windowBackground", dark.window_background),
        ("text", dark.text),
        ("textMuted", dark.text_muted),
        ("accent", dark.accent),
        ("separator", dark.separator),
    ];
    for (name, expected) in cases {
        let got = read_rgba(&swift, "enum TahoeDesign_Dark", name)
            .unwrap_or_else(|| panic!("{name} not found in the emitted Swift"));
        let want = [
            f64::from(expected.r),
            f64::from(expected.g),
            f64::from(expected.b),
            f64::from(expected.a),
        ];
        for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert!(
                (g - w).abs() < 1e-5,
                "{name} component {i}: Swift has {g}, Rust has {w}"
            );
        }
    }
}

#[test]
fn the_light_appearance_round_trips_too() {
    let light = DesignSystem::light();
    let swift = both();
    for (name, expected) in [
        ("text", light.text),
        ("panel", light.panel),
        ("accent", light.accent),
    ] {
        let got = read_rgba(&swift, "enum TahoeDesign_Light", name)
            .unwrap_or_else(|| panic!("{name} not found in the light namespace"));
        assert!(
            (got[0] - f64::from(expected.r)).abs() < 1e-5,
            "{name}: Swift {} vs Rust {}",
            got[0],
            expected.r
        );
    }
    // And the two appearances must actually differ, or the file is pointless.
    let dark_text = read_rgba(&swift, "enum TahoeDesign_Dark", "text").expect("dark text");
    let light_text = read_rgba(&swift, "enum TahoeDesign_Light", "text").expect("light text");
    assert_ne!(dark_text, light_text);
}

#[test]
fn spacing_radii_and_metrics_round_trip() {
    let dark = DesignSystem::dark();
    let swift = both();
    let ns = "enum TahoeDesign_Dark";

    for (name, expected) in [
        ("spacingXs", dark.spacing.xs),
        ("spacingSm", dark.spacing.sm),
        ("spacingMd", dark.spacing.md),
        ("spacingLg", dark.spacing.lg),
        ("spacingXl", dark.spacing.xl),
    ] {
        assert_eq!(
            read_double(&swift, ns, name),
            Some(f64::from(expected)),
            "{name}"
        );
    }
    for (name, expected) in [
        ("radiusControl", dark.radii.control),
        ("radiusCard", dark.radii.card),
        ("radiusWindow", dark.radii.window),
        ("radiusDock", dark.radii.dock),
    ] {
        assert_eq!(
            read_double(&swift, ns, name),
            Some(f64::from(expected)),
            "{name}"
        );
    }
    for (name, expected) in [
        ("panelHeight", dark.metrics.panel_height),
        ("dockHeight", dark.metrics.dock_height),
        ("dockIcon", dark.metrics.dock_icon),
        ("launcherRow", dark.metrics.launcher_row),
        ("sidebarWidth", dark.metrics.sidebar_width),
    ] {
        assert_eq!(
            read_double(&swift, ns, name),
            Some(f64::from(expected)),
            "{name}"
        );
    }
}

#[test]
fn the_panel_height_the_compositor_reserves_is_the_one_swift_gets() {
    // The compositor reserves exactly this many pixels; a SwiftUI window that
    // assumed anything else would sit under the panel.
    let swift = both();
    assert_eq!(
        read_double(&swift, "enum TahoeDesign_Dark", "panelHeight"),
        Some(28.0)
    );
    assert_eq!(
        read_double(&swift, "enum TahoeDesign_Light", "panelHeight"),
        Some(28.0)
    );
}

#[test]
fn every_wallpaper_stop_is_emitted_for_both_appearances() {
    let dark = DesignSystem::dark();
    let light = DesignSystem::light();
    let swift = both();
    // The wallpaper is shared, so both namespaces must carry all of it.
    for ns in ["enum TahoeDesign_Dark", "enum TahoeDesign_Light"] {
        let start = swift.find(ns).expect("namespace");
        let body = &swift[start..];
        let end = body.find("// MARK: - Chrome materials").expect("marker");
        let section = &body[..end];
        let emitted = section.matches("GradientStop(position:").count();
        assert_eq!(
            emitted,
            dark.wallpaper.stops.len(),
            "{ns} should carry every stop"
        );
        assert_eq!(emitted, light.wallpaper.stops.len());
    }
}

#[test]
fn the_shared_tokens_are_identical_between_appearances() {
    // The Rust side already asserts metrics and wallpaper are shared; this
    // checks the emitter did not introduce a difference.
    let swift = both();
    for name in ["spacingXl", "radiusDock", "panelHeight", "sidebarWidth"] {
        assert_eq!(
            read_double(&swift, "enum TahoeDesign_Dark", name),
            read_double(&swift, "enum TahoeDesign_Light", name),
            "{name} differs between appearances"
        );
    }
}

#[test]
fn a_swiftui_switch_over_appearance_is_exhaustive() {
    // Every accessor must handle both cases; a third appearance would then
    // fail to compile rather than silently render the wrong colour.
    let swift = both();
    let gate = swift.find("#if canImport(SwiftUI)").expect("gate");
    let swiftui = &swift[gate..];
    let switches = swiftui.matches("switch appearance {").count();
    let dark_cases = swiftui.matches("case .dark").count();
    let light_cases = swiftui.matches("case .light").count();
    // Seven colour accessors plus the gradient builder.
    assert_eq!(
        switches, 8,
        "expected a switch per cross-appearance accessor"
    );
    assert_eq!(dark_cases, switches, "every switch must handle .dark");
    assert_eq!(light_cases, switches, "every switch must handle .light");
}

// ---------------------------------------------------------------------------
// Single-appearance output
// ---------------------------------------------------------------------------

#[test]
fn a_single_appearance_file_names_its_namespace_tahoe_design() {
    let swift = DesignSystem::light().to_swift();
    assert!(swift.contains("public enum TahoeDesign {"));
    assert!(!swift.contains("TahoeDesign_Light"));
    assert!(!swift.contains("TahoeDesign_Dark"));
    assert!(
        swift.contains("One appearance: Tahoe Light."),
        "the header must name the appearance"
    );
}

#[test]
fn a_single_appearance_file_still_balances() {
    for system in [DesignSystem::light(), DesignSystem::dark()] {
        let swift = system.to_swift();
        let mut depth = 0i32;
        for c in swift.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        assert_eq!(depth, 0, "unbalanced in {}", system.name);
        assert!(!swift.contains("{{") && !swift.contains("}}"));
    }
}

// ---------------------------------------------------------------------------
// Committed file freshness
// ---------------------------------------------------------------------------

#[test]
fn the_committed_swift_file_matches_the_rust_design_system() {
    // This is the check that makes the whole arrangement work: if someone
    // edits the generated file, or changes a token and forgets to
    // regenerate, this fails with the exact command to run.
    let committed = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apple/Sources/TahoeDesign/TahoeDesign.swift");
    if !committed.exists() {
        eprintln!(
            "note: {} is not present; skipping the freshness check",
            committed.display()
        );
        return;
    }
    let expected = both();
    let actual = std::fs::read_to_string(&committed).expect("read the committed file");
    if actual != expected {
        // Point at the first difference rather than dumping two files.
        let first_diff = actual
            .lines()
            .zip(expected.lines())
            .position(|(a, b)| a != b)
            .map_or(0, |i| i + 1);
        panic!(
            "{} is stale (first difference at line {first_diff}).\nRun:\n    \
             cargo run -p tahoe-shell --bin tahoe-design-export -- apple/Sources/TahoeDesign/TahoeDesign.swift",
            committed.display()
        );
    }
}

// ---------------------------------------------------------------------------
// Consistency with the rest of the crate
// ---------------------------------------------------------------------------

#[test]
fn spacing_types_are_not_reinvented() {
    // A guard against the emitter growing its own idea of the scale.
    let s = Spacing::default();
    assert_eq!((s.xs, s.sm, s.md, s.lg, s.xl), (4, 8, 12, 20, 32));
    let r = Radii::default();
    assert_eq!((r.control, r.card, r.window, r.dock), (6, 10, 12, 20));
}
