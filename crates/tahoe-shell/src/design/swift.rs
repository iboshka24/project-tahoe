// SPDX-License-Identifier: MIT OR Apache-2.0
//! Swift emission for the design system.
//!
//! # Why generate Swift from Rust
//!
//! Tahoe has two front ends that must look identical: the Wayland shell
//! (Rust, rendering through `tahoe-shell::preview` and the compositor) and
//! the Apple-side application under `apple/` (SwiftUI, built with xtool).
//!
//! Two hand-written copies of a palette diverge. Always. The first
//! "temporary" tweak to one of them breaks the test that compares them, and
//! the fix is to loosen the test, and then nobody notices when the accent
//! colour stops matching.
//!
//! So the Rust [`DesignSystem`] is the single source of truth and this
//! module *emits* the Swift. Divergence stops being a discipline problem
//! and becomes structurally impossible.
//!
//! # Shape of the output
//!
//! One file, `TahoeDesign.swift`, containing:
//!
//! - `RGBA` / `GradientStop` value types — pure Swift, no imports.
//! - `TahoeDesign_Dark` and `TahoeDesign_Light` — the raw tokens for each
//!   appearance, also import-free, so the values can be asserted by a
//!   headless `swift test` rather than only by an iPhone build.
//! - `Tahoe` — the resolver the application actually uses.
//! - `#if canImport(SwiftUI)` extensions adding `Color`, `LinearGradient`
//!   and `CGFloat` conveniences.
//!
//! Generation is deterministic, so a committed copy can be checked by
//! regenerating and comparing bytes. That check lives in the tests, and it
//! fails if the file on disk is stale.

use std::fmt::Write as _;

use super::{DesignSystem, Gradient, Rgba};

/// Format a float the way Swift wants it.
///
/// `{:.6}` is trimmed of trailing zeros to keep the diff readable, but the
/// trailing `.0` is put back: Swift would read a bare `1` as `Int` and
/// refuse to compile it into a `Double` field.
#[must_use]
pub fn swift_float(value: f32) -> String {
    let mut s = format!("{value:.6}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.push('0');
        }
    }
    if s == "-0.0" {
        s = "0.0".to_owned();
    }
    s
}

fn rgba(c: Rgba) -> String {
    format!(
        "RGBA(red: {}, green: {}, blue: {}, alpha: {})",
        swift_float(c.r),
        swift_float(c.g),
        swift_float(c.b),
        swift_float(c.a)
    )
}

fn gradient(g: &Gradient) -> String {
    let mut out = String::from("[\n");
    for stop in &g.stops {
        let _ = writeln!(
            out,
            "            GradientStop(position: {}, color: {}),",
            swift_float(stop.at),
            rgba(stop.color)
        );
    }
    out.push_str("        ]");
    out
}

/// The shared, appearance-independent part of the file.
const VALUE_TYPES: &str = r#"/// A straight (non-premultiplied) sRGB colour.
///
/// Components are 0...1. Deliberately not `SwiftUI.Color`, so the tokens can
/// be read and asserted without importing a UI framework.
public struct RGBA: Equatable, Sendable {
    public let red: Double
    public let green: Double
    public let blue: Double
    public let alpha: Double

    public init(red: Double, green: Double, blue: Double, alpha: Double) {
        self.red = red
        self.green = green
        self.blue = blue
        self.alpha = alpha
    }

    /// The same colour at a different opacity.
    public func withAlpha(_ value: Double) -> RGBA {
        RGBA(red: red, green: green, blue: blue, alpha: value)
    }
}

/// One position in a vertical gradient; 0 is the top, 1 the bottom.
public struct GradientStop: Equatable, Sendable {
    public let position: Double
    public let color: RGBA

    public init(position: Double, color: RGBA) {
        self.position = position
        self.color = color
    }
}
"#;

/// Emit a token namespace for one appearance.
///
/// `namespace` is the Swift enum name, which differs per appearance so both
/// can live in one file.
fn emit_tokens(system: &DesignSystem, namespace: &str, wallpaper_fn: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "/// Design tokens for the {} appearance.", system.name);
    let _ = writeln!(out, "public enum {namespace} {{");
    let _ = writeln!(
        out,
        "    public static let appearanceName = {:?}",
        system.name
    );
    let _ = writeln!(out, "    public static let isDark = {}", system.is_dark);

    let _ = writeln!(
        out,
        "\n    // MARK: - Wallpaper\n    /// Vertical gradient, top to bottom."
    );
    let _ = writeln!(
        out,
        "    public static let wallpaper: [GradientStop] = {};",
        gradient(&system.wallpaper)
    );

    let _ = writeln!(out, "\n    // MARK: - Chrome materials");
    let _ = writeln!(out, "    /// Top panel material. Translucent.");
    let _ = writeln!(out, "    public static let panel = {}", rgba(system.panel));
    let _ = writeln!(out, "    /// Dock material. Translucent.");
    let _ = writeln!(out, "    public static let dock = {}", rgba(system.dock));
    let _ = writeln!(
        out,
        "    /// Background for windows that draw none of their own."
    );
    let _ = writeln!(
        out,
        "    public static let windowBackground = {}",
        rgba(system.window_background)
    );

    let _ = writeln!(out, "\n    // MARK: - Content");
    let _ = writeln!(out, "    public static let text = {}", rgba(system.text));
    let _ = writeln!(
        out,
        "    public static let textMuted = {}",
        rgba(system.text_muted)
    );
    let _ = writeln!(
        out,
        "    public static let accent = {}",
        rgba(system.accent)
    );
    let _ = writeln!(
        out,
        "    public static let separator = {}",
        rgba(system.separator)
    );

    let _ = writeln!(out, "\n    // MARK: - Spacing (points)");
    for (name, value) in [
        ("xs", system.spacing.xs),
        ("sm", system.spacing.sm),
        ("md", system.spacing.md),
        ("lg", system.spacing.lg),
        ("xl", system.spacing.xl),
    ] {
        let _ = writeln!(
            out,
            "    public static let spacing{}: Double = {value}",
            capitalize(name)
        );
    }

    let _ = writeln!(out, "\n    // MARK: - Corner radii (points)");
    for (name, value) in [
        ("control", system.radii.control),
        ("card", system.radii.card),
        ("window", system.radii.window),
        ("dock", system.radii.dock),
    ] {
        let _ = writeln!(
            out,
            "    public static let radius{}: Double = {value}",
            capitalize(name)
        );
    }

    let _ = writeln!(out, "\n    // MARK: - Metrics (points)");
    for (name, value) in [
        ("panelHeight", system.metrics.panel_height),
        ("dockHeight", system.metrics.dock_height),
        ("dockIcon", system.metrics.dock_icon),
        ("launcherRow", system.metrics.launcher_row),
        ("sidebarWidth", system.metrics.sidebar_width),
    ] {
        let _ = writeln!(out, "    public static let {name}: Double = {value}");
    }

    // The sampler. Mirrors `DesignSystem::wallpaper_at`; the Rust side is
    // the reference and a test compares them.
    let _ = writeln!(
        out,
        r#"
    /// The wallpaper sampled at `t`, clamped to 0...1.
    ///
    /// Mirrors `DesignSystem::wallpaper_at`. The Rust implementation is the
    /// reference; `swift_emitter` tests assert this matches it.
    public static func {wallpaper_fn}(at t: Double) -> RGBA {{
        let clamped = min(max(t, 0), 1)
        guard let first = wallpaper.first else {{
            return RGBA(red: 0, green: 0, blue: 0, alpha: 1)
        }}
        if clamped <= first.position {{ return first.color }}
        var previous = first
        for stop in wallpaper.dropFirst() {{
            if clamped <= stop.position {{
                let span = stop.position - previous.position
                let local = span <= Double.ulpOfOne ? 1 : (clamped - previous.position) / span
                return RGBA(
                    red: previous.color.red + (stop.color.red - previous.color.red) * local,
                    green: previous.color.green + (stop.color.green - previous.color.green) * local,
                    blue: previous.color.blue + (stop.color.blue - previous.color.blue) * local,
                    alpha: previous.color.alpha + (stop.color.alpha - previous.color.alpha) * local
                )
            }}
            previous = stop
        }}
        return previous.color
    }}
}}"#
    );
    out.push('\n');
    out
}

/// The `#if canImport(SwiftUI)` conveniences.
///
/// Kept to plain `switch` statements rather than a generic abstraction: the
/// two token namespaces are concrete, the switch is exhaustive, and a third
/// appearance added later will fail to compile until every accessor handles
/// it. That is the behaviour we want.
fn emit_swiftui_extensions() -> String {
    let accessors = [
        ("panelColor", "panel"),
        ("dockColor", "dock"),
        ("windowBackgroundColor", "windowBackground"),
        ("textColor", "text"),
        ("textMutedColor", "textMuted"),
        ("accentColor", "accent"),
        ("separatorColor", "separator"),
    ];
    let mut out = String::from(
        "\n#if canImport(SwiftUI)\nimport SwiftUI\n\npublic extension RGBA {\n    /// This colour as a SwiftUI `Color`.\n    var color: Color { Color(red: red, green: green, blue: blue, opacity: alpha) }\n}\n\n",
    );
    out.push_str("public extension Tahoe {\n");
    for (name, field) in accessors {
        let _ = writeln!(
            out,
            "    /// The `{field}` token for an appearance.\n    static func {name}(_ appearance: TahoeAppearance) -> Color {{\n        switch appearance {{\n        case .dark: return TahoeDesign_Dark.{field}.color\n        case .light: return TahoeDesign_Light.{field}.color\n        }}\n    }}\n"
        );
    }
    out.push_str(SWIFTUI_STATIC_BLOCK);
    out
}

impl DesignSystem {
    /// Render a single-appearance Swift file.
    ///
    /// Used by the tests, which check individual values; the application
    /// imports [`DesignSystem::to_swift_with_appearances`] instead.
    #[must_use]
    pub fn to_swift(&self) -> String {
        let header = self.swift_header(false);
        format!(
            "{header}\n{VALUE_TYPES}\n{}\n{}",
            emit_tokens(self, "TahoeDesign", "wallpaperColor"),
            emit_swiftui_extensions_single()
        )
    }

    /// Render both appearances into the one file the application imports.
    #[must_use]
    pub fn to_swift_with_appearances(&self, other: &Self) -> String {
        // The dark appearance is the reference for shared values. The
        // wallpaper and metrics are identical between appearances — there
        // is a Rust test enforcing that — so either could be used.
        let (light, dark) = if self.is_dark {
            (other, self)
        } else {
            (self, other)
        };
        let mut out = self.swift_header(true);
        out.push('\n');
        out.push_str(VALUE_TYPES);
        out.push('\n');
        out.push_str(&emit_tokens(dark, "TahoeDesign_Dark", "darkWallpaperColor"));
        out.push('\n');
        out.push_str(&emit_tokens(
            light,
            "TahoeDesign_Light",
            "lightWallpaperColor",
        ));
        let _ = writeln!(
            out,
            r#"
/// Which appearance the interface should use.
public enum TahoeAppearance: String, CaseIterable, Sendable {{
    case light
    case dark
}}

/// The resolved design system.
///
/// Layout metrics and the wallpaper are shared between appearances; only the
/// chrome colours differ. A Rust test enforces that, so a change that shifted
/// layout between light and dark would fail the build.
public enum Tahoe {{
    /// The appearance the desktop starts with.
    ///
    /// Dark, because the Settings portal that would report the user's real
    /// preference is not implemented on the Tahoe side yet. This is a
    /// documented default, not a guess.
    public static let defaultAppearance: TahoeAppearance = .dark

    /// The wallpaper sampled at `t` for an appearance.
    public static func wallpaper(at t: Double, appearance: TahoeAppearance) -> RGBA {{
        switch appearance {{
        case .dark: return TahoeDesign_Dark.darkWallpaperColor(at: t)
        case .light: return TahoeDesign_Light.lightWallpaperColor(at: t)
        }}
    }}

    /// Logical points per inch, for converting the point-based tokens to
    /// physical sizes when a platform needs that.
    public static let pointsPerInch: Double = 72
}}
"#
        );
        let _ = writeln!(out, "{}", emit_swiftui_extensions());
        out
    }

    fn swift_header(&self, both: bool) -> String {
        let regenerate = "cargo run -p tahoe-shell --bin tahoe-design-export -- apple/Sources/TahoeDesign/TahoeDesign.swift";
        let scope = if both {
            format!(
                "Both appearances. Dark is {}. Light is {}.",
                self.name, self.name
            )
        } else {
            format!("One appearance: {}.", self.name)
        };
        format!(
            r#"// Generated from crates/tahoe-shell/src/design.rs — DO NOT EDIT BY HAND.
//
// Regenerate with:
//     {regenerate}
//
// The Rust design system is the single source of truth. The `swift_emitter`
// tests regenerate this file and fail if the committed copy has drifted, so
// editing it here only produces a failing build.
//
// Scope: {scope}
"#
        )
    }
}

/// The SwiftUI extension for a single-appearance file.
///
/// A separate emitter rather than a parameter of the two-appearance one,
/// because the shape differs: there is no appearance to switch on, so the
/// accessors are plain properties. Emitting the two-namespace version here
/// was a real bug — the generated file referenced `TahoeDesign_Dark`, which
/// a single-appearance file does not define, and would not have compiled.
fn emit_swiftui_extensions_single() -> String {
    let accessors = [
        ("panelColor", "panel"),
        ("dockColor", "dock"),
        ("windowBackgroundColor", "windowBackground"),
        ("textColor", "text"),
        ("textMutedColor", "textMuted"),
        ("accentColor", "accent"),
        ("separatorColor", "separator"),
    ];
    let mut out = String::from(
        "\n#if canImport(SwiftUI)\nimport SwiftUI\n\npublic extension RGBA {\n    /// This colour as a SwiftUI `Color`.\n    var color: Color { Color(red: red, green: green, blue: blue, opacity: alpha) }\n}\n\npublic extension TahoeDesign {\n",
    );
    for (name, field) in accessors {
        let _ = writeln!(
            out,
            "    /// The `{field}` token as a `Color`.\n    static var {name}: Color {{ {field}.color }}\n"
        );
    }
    out.push_str(
        r#"    /// The wallpaper as a gradient, ready for a `View` background.
    static var wallpaperGradient: LinearGradient {
        LinearGradient(
            stops: wallpaper.map { Gradient.Stop(color: $0.color.color, location: $0.position) },
            startPoint: .top,
            endPoint: .bottom
        )
    }

    // MARK: - Tokens as CGFloat

    /// A spacing step, as a `CGFloat`.
    static func spacing(_ step: SpacingStep) -> CGFloat {
        switch step {
        case .xs: return CGFloat(spacingXs)
        case .sm: return CGFloat(spacingSm)
        case .md: return CGFloat(spacingMd)
        case .lg: return CGFloat(spacingLg)
        case .xl: return CGFloat(spacingXl)
        }
    }

    /// A corner radius, as a `CGFloat`.
    static func radius(_ size: RadiusSize) -> CGFloat {
        switch size {
        case .control: return CGFloat(radiusControl)
        case .card: return CGFloat(radiusCard)
        case .window: return CGFloat(radiusWindow)
        case .dock: return CGFloat(radiusDock)
        }
    }
}

/// A step on the spacing scale.
public enum SpacingStep: String, CaseIterable, Sendable {
    case xs, sm, md, lg, xl
}

/// A corner radius from the design system.
public enum RadiusSize: String, CaseIterable, Sendable {
    case control, card, window, dock
}
#endif
"#,
    );
    out
}

/// The parts of the SwiftUI extension that need no substitution.
///
/// Held as a plain string so Swift's braces never meet Rust's format
/// machinery — `r#"…"#` is still a format string when passed to `writeln!`,
/// and every `{` in the Swift closure became a placeholder the first time
/// this was written.
const SWIFTUI_STATIC_BLOCK: &str = r#"
    /// The wallpaper as a gradient, ready for a `View` background.
    static func wallpaperGradient(_ appearance: TahoeAppearance) -> LinearGradient {
        let stops: [Gradient.Stop]
        switch appearance {
        case .dark:
            stops = TahoeDesign_Dark.wallpaper.map {
                Gradient.Stop(color: $0.color.color, location: $0.position)
            }
        case .light:
            stops = TahoeDesign_Light.wallpaper.map {
                Gradient.Stop(color: $0.color.color, location: $0.position)
            }
        }
        return LinearGradient(stops: stops, startPoint: .top, endPoint: .bottom)
    }

    // MARK: - Tokens as CGFloat

    /// A spacing step, as a `CGFloat`.
    static func spacing(_ step: SpacingStep, _ appearance: TahoeAppearance) -> CGFloat {
        CGFloat(spacingValue(step, dark: appearance == .dark))
    }

    private static func spacingValue(_ step: SpacingStep, dark: Bool) -> Double {
        switch step {
        case .xs: return dark ? TahoeDesign_Dark.spacingXs : TahoeDesign_Light.spacingXs
        case .sm: return dark ? TahoeDesign_Dark.spacingSm : TahoeDesign_Light.spacingSm
        case .md: return dark ? TahoeDesign_Dark.spacingMd : TahoeDesign_Light.spacingMd
        case .lg: return dark ? TahoeDesign_Dark.spacingLg : TahoeDesign_Light.spacingLg
        case .xl: return dark ? TahoeDesign_Dark.spacingXl : TahoeDesign_Light.spacingXl
        }
    }

    /// A corner radius, as a `CGFloat`.
    static func radius(_ size: RadiusSize, _ appearance: TahoeAppearance) -> CGFloat {
        CGFloat(radiusValue(size, dark: appearance == .dark))
    }

    private static func radiusValue(_ size: RadiusSize, dark: Bool) -> Double {
        switch size {
        case .control: return dark ? TahoeDesign_Dark.radiusControl : TahoeDesign_Light.radiusControl
        case .card: return dark ? TahoeDesign_Dark.radiusCard : TahoeDesign_Light.radiusCard
        case .window: return dark ? TahoeDesign_Dark.radiusWindow : TahoeDesign_Light.radiusWindow
        case .dock: return dark ? TahoeDesign_Dark.radiusDock : TahoeDesign_Light.radiusDock
        }
    }

    /// The panel height, as a `CGFloat`.
    static func panelHeight(_ appearance: TahoeAppearance) -> CGFloat {
        CGFloat(appearance == .dark ? TahoeDesign_Dark.panelHeight : TahoeDesign_Light.panelHeight)
    }

    /// The dock height, as a `CGFloat`.
    static func dockHeight(_ appearance: TahoeAppearance) -> CGFloat {
        CGFloat(appearance == .dark ? TahoeDesign_Dark.dockHeight : TahoeDesign_Light.dockHeight)
    }

    /// A dock icon's edge length, as a `CGFloat`.
    static func dockIconSize(_ appearance: TahoeAppearance) -> CGFloat {
        CGFloat(appearance == .dark ? TahoeDesign_Dark.dockIcon : TahoeDesign_Light.dockIcon)
    }
}

/// A step on the spacing scale.
public enum SpacingStep: String, CaseIterable, Sendable {
    case xs, sm, md, lg, xl
}

/// A corner radius from the design system.
public enum RadiusSize: String, CaseIterable, Sendable {
    case control, card, window, dock
}
#endif
"#;

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests;
