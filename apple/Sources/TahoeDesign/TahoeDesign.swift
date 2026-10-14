// Generated from crates/tahoe-shell/src/design.rs — DO NOT EDIT BY HAND.
//
// Regenerate with:
//     cargo run -p tahoe-shell --bin tahoe-design-export -- apple/Sources/TahoeDesign/TahoeDesign.swift
//
// The Rust design system is the single source of truth. The `swift_emitter`
// tests regenerate this file and fail if the committed copy has drifted, so
// editing it here only produces a failing build.
//
// Scope: Both appearances. Dark is Tahoe Dark. Light is Tahoe Dark.

/// A straight (non-premultiplied) sRGB colour.
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

/// Design tokens for the Tahoe Dark appearance.
public enum TahoeDesign_Dark {
    public static let appearanceName = "Tahoe Dark"
    public static let isDark = true

    // MARK: - Wallpaper
    /// Vertical gradient, top to bottom.
    public static let wallpaper: [GradientStop] = [
            GradientStop(position: 0.0, color: RGBA(red: 0.070588, green: 0.101961, blue: 0.227451, alpha: 1.0)),
            GradientStop(position: 0.28, color: RGBA(red: 0.172549, green: 0.164706, blue: 0.431373, alpha: 1.0)),
            GradientStop(position: 0.55, color: RGBA(red: 0.352941, green: 0.247059, blue: 0.607843, alpha: 1.0)),
            GradientStop(position: 0.8, color: RGBA(red: 0.705882, green: 0.388235, blue: 0.54902, alpha: 1.0)),
            GradientStop(position: 1.0, color: RGBA(red: 0.941176, green: 0.658824, blue: 0.478431, alpha: 1.0)),
        ];

    // MARK: - Chrome materials
    /// Top panel material. Translucent.
    public static let panel = RGBA(red: 0.109804, green: 0.109804, blue: 0.133333, alpha: 0.66)
    /// Dock material. Translucent.
    public static let dock = RGBA(red: 0.164706, green: 0.164706, blue: 0.2, alpha: 0.62)
    /// Background for windows that draw none of their own.
    public static let windowBackground = RGBA(red: 0.117647, green: 0.117647, blue: 0.141176, alpha: 1.0)

    // MARK: - Content
    public static let text = RGBA(red: 0.960784, green: 0.960784, blue: 0.968627, alpha: 1.0)
    public static let textMuted = RGBA(red: 0.960784, green: 0.960784, blue: 0.968627, alpha: 0.55)
    public static let accent = RGBA(red: 0.039216, green: 0.517647, blue: 1.0, alpha: 1.0)
    public static let separator = RGBA(red: 1.0, green: 1.0, blue: 1.0, alpha: 0.12)

    // MARK: - Spacing (points)
    public static let spacingXs: Double = 4
    public static let spacingSm: Double = 8
    public static let spacingMd: Double = 12
    public static let spacingLg: Double = 20
    public static let spacingXl: Double = 32

    // MARK: - Corner radii (points)
    public static let radiusControl: Double = 6
    public static let radiusCard: Double = 10
    public static let radiusWindow: Double = 12
    public static let radiusDock: Double = 20

    // MARK: - Metrics (points)
    public static let panelHeight: Double = 28
    public static let dockHeight: Double = 64
    public static let dockIcon: Double = 48
    public static let launcherRow: Double = 36
    public static let sidebarWidth: Double = 176

    /// The wallpaper sampled at `t`, clamped to 0...1.
    ///
    /// Mirrors `DesignSystem::wallpaper_at`. The Rust implementation is the
    /// reference; `swift_emitter` tests assert this matches it.
    public static func wallpaperColor(at t: Double) -> RGBA {
        let clamped = min(max(t, 0), 1)
        guard let first = wallpaper.first else {
            return RGBA(red: 0, green: 0, blue: 0, alpha: 1)
        }
        if clamped <= first.position { return first.color }
        var previous = first
        for stop in wallpaper.dropFirst() {
            if clamped <= stop.position {
                let span = stop.position - previous.position
                let local = span <= Double.ulpOfOne ? 1 : (clamped - previous.position) / span
                return RGBA(
                    red: previous.color.red + (stop.color.red - previous.color.red) * local,
                    green: previous.color.green + (stop.color.green - previous.color.green) * local,
                    blue: previous.color.blue + (stop.color.blue - previous.color.blue) * local,
                    alpha: previous.color.alpha + (stop.color.alpha - previous.color.alpha) * local
                )
            }
            previous = stop
        }
        return previous.color
    }
}


/// Design tokens for the Tahoe Light appearance.
public enum TahoeDesign_Light {
    public static let appearanceName = "Tahoe Light"
    public static let isDark = false

    // MARK: - Wallpaper
    /// Vertical gradient, top to bottom.
    public static let wallpaper: [GradientStop] = [
            GradientStop(position: 0.0, color: RGBA(red: 0.070588, green: 0.101961, blue: 0.227451, alpha: 1.0)),
            GradientStop(position: 0.28, color: RGBA(red: 0.172549, green: 0.164706, blue: 0.431373, alpha: 1.0)),
            GradientStop(position: 0.55, color: RGBA(red: 0.352941, green: 0.247059, blue: 0.607843, alpha: 1.0)),
            GradientStop(position: 0.8, color: RGBA(red: 0.705882, green: 0.388235, blue: 0.54902, alpha: 1.0)),
            GradientStop(position: 1.0, color: RGBA(red: 0.941176, green: 0.658824, blue: 0.478431, alpha: 1.0)),
        ];

    // MARK: - Chrome materials
    /// Top panel material. Translucent.
    public static let panel = RGBA(red: 0.94902, green: 0.94902, blue: 0.968627, alpha: 0.72)
    /// Dock material. Translucent.
    public static let dock = RGBA(red: 0.968627, green: 0.968627, blue: 0.980392, alpha: 0.68)
    /// Background for windows that draw none of their own.
    public static let windowBackground = RGBA(red: 1.0, green: 1.0, blue: 1.0, alpha: 1.0)

    // MARK: - Content
    public static let text = RGBA(red: 0.109804, green: 0.109804, blue: 0.117647, alpha: 1.0)
    public static let textMuted = RGBA(red: 0.109804, green: 0.109804, blue: 0.117647, alpha: 0.55)
    public static let accent = RGBA(red: 0.039216, green: 0.517647, blue: 1.0, alpha: 1.0)
    public static let separator = RGBA(red: 0.235294, green: 0.235294, blue: 0.262745, alpha: 0.18)

    // MARK: - Spacing (points)
    public static let spacingXs: Double = 4
    public static let spacingSm: Double = 8
    public static let spacingMd: Double = 12
    public static let spacingLg: Double = 20
    public static let spacingXl: Double = 32

    // MARK: - Corner radii (points)
    public static let radiusControl: Double = 6
    public static let radiusCard: Double = 10
    public static let radiusWindow: Double = 12
    public static let radiusDock: Double = 20

    // MARK: - Metrics (points)
    public static let panelHeight: Double = 28
    public static let dockHeight: Double = 64
    public static let dockIcon: Double = 48
    public static let launcherRow: Double = 36
    public static let sidebarWidth: Double = 176

    /// The wallpaper sampled at `t`, clamped to 0...1.
    ///
    /// Mirrors `DesignSystem::wallpaper_at`. The Rust implementation is the
    /// reference; `swift_emitter` tests assert this matches it.
    public static func wallpaperColor(at t: Double) -> RGBA {
        let clamped = min(max(t, 0), 1)
        guard let first = wallpaper.first else {
            return RGBA(red: 0, green: 0, blue: 0, alpha: 1)
        }
        if clamped <= first.position { return first.color }
        var previous = first
        for stop in wallpaper.dropFirst() {
            if clamped <= stop.position {
                let span = stop.position - previous.position
                let local = span <= Double.ulpOfOne ? 1 : (clamped - previous.position) / span
                return RGBA(
                    red: previous.color.red + (stop.color.red - previous.color.red) * local,
                    green: previous.color.green + (stop.color.green - previous.color.green) * local,
                    blue: previous.color.blue + (stop.color.blue - previous.color.blue) * local,
                    alpha: previous.color.alpha + (stop.color.alpha - previous.color.alpha) * local
                )
            }
            previous = stop
        }
        return previous.color
    }
}


/// Which appearance the interface should use.
public enum TahoeAppearance: String, CaseIterable, Sendable {
    case light
    case dark
}

/// The resolved design system.
///
/// Layout metrics and the wallpaper are shared between appearances; only the
/// chrome colours differ. A Rust test enforces that, so a change that shifted
/// layout between light and dark would fail the build.
public enum Tahoe {
    /// The appearance the desktop starts with.
    ///
    /// Dark, because the Settings portal that would report the user's real
    /// preference is not implemented on the Tahoe side yet. This is a
    /// documented default, not a guess.
    public static let defaultAppearance: TahoeAppearance = .dark

    /// The wallpaper sampled at `t` for an appearance.
    public static func wallpaper(at t: Double, appearance: TahoeAppearance) -> RGBA {
        switch appearance {
        case .dark: return TahoeDesign_Dark.wallpaperColor(at: t)
        case .light: return TahoeDesign_Light.wallpaperColor(at: t)
        }
    }

    /// Logical points per inch, for converting the point-based tokens to
    /// physical sizes when a platform needs that.
    public static let pointsPerInch: Double = 72
}


#if canImport(SwiftUI)
import SwiftUI

public extension RGBA {
    /// This colour as a SwiftUI `Color`.
    var color: Color { Color(red: red, green: green, blue: blue, opacity: alpha) }
}

public extension Tahoe {
    /// The `panel` token for an appearance.
    static func panelColor(_ appearance: TahoeAppearance) -> Color {
        switch appearance {
        case .dark: return TahoeDesign_Dark.panel.color
        case .light: return TahoeDesign_Light.panel.color
        }
    }

    /// The `dock` token for an appearance.
    static func dockColor(_ appearance: TahoeAppearance) -> Color {
        switch appearance {
        case .dark: return TahoeDesign_Dark.dock.color
        case .light: return TahoeDesign_Light.dock.color
        }
    }

    /// The `windowBackground` token for an appearance.
    static func windowBackgroundColor(_ appearance: TahoeAppearance) -> Color {
        switch appearance {
        case .dark: return TahoeDesign_Dark.windowBackground.color
        case .light: return TahoeDesign_Light.windowBackground.color
        }
    }

    /// The `text` token for an appearance.
    static func textColor(_ appearance: TahoeAppearance) -> Color {
        switch appearance {
        case .dark: return TahoeDesign_Dark.text.color
        case .light: return TahoeDesign_Light.text.color
        }
    }

    /// The `textMuted` token for an appearance.
    static func textMutedColor(_ appearance: TahoeAppearance) -> Color {
        switch appearance {
        case .dark: return TahoeDesign_Dark.textMuted.color
        case .light: return TahoeDesign_Light.textMuted.color
        }
    }

    /// The `accent` token for an appearance.
    static func accentColor(_ appearance: TahoeAppearance) -> Color {
        switch appearance {
        case .dark: return TahoeDesign_Dark.accent.color
        case .light: return TahoeDesign_Light.accent.color
        }
    }

    /// The `separator` token for an appearance.
    static func separatorColor(_ appearance: TahoeAppearance) -> Color {
        switch appearance {
        case .dark: return TahoeDesign_Dark.separator.color
        case .light: return TahoeDesign_Light.separator.color
        }
    }


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

