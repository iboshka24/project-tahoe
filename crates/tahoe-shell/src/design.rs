// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tahoe design system.
//!
//! One place defines every colour, radius, spacing step and type size the
//! desktop uses. The compositor paints its backdrop from here, and the
//! shell reads the same tokens, so the two can never drift apart.
//!
//! The palette follows the current macOS "Tahoe"-era look: a soft
//! translucent material over a wide wallpaper gradient, strong corner
//! radii on floating surfaces, and a restrained accent colour. Values are
//! plain numbers rather than a framework, so they are usable from a
//! renderer with no UI toolkit and are trivially unit-testable.

/// Straight (non-premultiplied) RGBA colour in linear sRGB components.
///
/// Deliberately a tiny type with no colour-space conversion: the renderer
/// and the shell both want plain floats, and a conversion layer nobody asks
/// for is a bug surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    /// Red, 0.0..=1.0.
    pub r: f32,
    /// Green, 0.0..=1.0.
    pub g: f32,
    /// Blue, 0.0..=1.0.
    pub b: f32,
    /// Alpha, 0.0..=1.0.
    pub a: f32,
}

impl Rgba {
    /// Build a colour, clamping components into range.
    #[must_use]
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r: r.clamp(0.0, 1.0),
            g: g.clamp(0.0, 1.0),
            b: b.clamp(0.0, 1.0),
            a: a.clamp(0.0, 1.0),
        }
    }

    /// Build an opaque colour from 8-bit components, the form designers
    /// and hex codes use.
    #[must_use]
    pub fn rgb8(r: u8, g: u8, b: u8) -> Self {
        Self::new(
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
            1.0,
        )
    }

    /// The same colour at a different opacity.
    #[must_use]
    pub fn with_alpha(self, a: f32) -> Self {
        Self {
            a: a.clamp(0.0, 1.0),
            ..self
        }
    }

    /// Linear interpolation towards `other`; `t` is clamped to 0..=1.
    #[must_use]
    pub fn mix(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: self.a + (other.a - self.a) * t,
        }
    }
}

/// A vertical gradient stop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stop {
    /// Position along the gradient, 0.0 = top, 1.0 = bottom.
    pub at: f32,
    /// Colour at that position.
    pub color: Rgba,
}

/// A multi-stop vertical gradient.
#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    /// Stops, ordered by [`Stop::at`].
    pub stops: Vec<Stop>,
}

impl Gradient {
    /// Sample the gradient at `t` (clamped), interpolating between stops.
    #[must_use]
    pub fn sample(&self, t: f32) -> Rgba {
        let t = t.clamp(0.0, 1.0);
        match self.stops.as_slice() {
            [] => Rgba::new(0.0, 0.0, 0.0, 1.0),
            [only] => only.color,
            [first, rest @ ..] => {
                if t <= first.at {
                    return first.color;
                }
                let mut prev = first;
                for stop in rest {
                    if t <= stop.at {
                        let span = stop.at - prev.at;
                        let local = if span <= f32::EPSILON {
                            1.0
                        } else {
                            (t - prev.at) / span
                        };
                        return prev.color.mix(stop.color, local);
                    }
                    prev = stop;
                }
                prev.color
            }
        }
    }
}

/// Spacing steps, in logical pixels. A 4px base scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spacing {
    /// 4px — tight gaps inside a control.
    pub xs: i32,
    /// 8px — default gap between related controls.
    pub sm: i32,
    /// 12px — padding inside a surface.
    pub md: i32,
    /// 20px — gap between groups.
    pub lg: i32,
    /// 32px — screen margins.
    pub xl: i32,
}

impl Default for Spacing {
    fn default() -> Self {
        Self {
            xs: 4,
            sm: 8,
            md: 12,
            lg: 20,
            xl: 32,
        }
    }
}

/// Corner radii, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Radii {
    /// Small controls (segmented buttons, badges).
    pub control: i32,
    /// Cards and menu items.
    pub card: i32,
    /// Floating windows.
    pub window: i32,
    /// Dock and panels.
    pub dock: i32,
}

impl Default for Radii {
    fn default() -> Self {
        Self {
            control: 6,
            card: 10,
            window: 12,
            dock: 20,
        }
    }
}

/// Metrics of the shell chrome, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metrics {
    /// Height of the top panel strip. Must match the compositor's reserved
    /// area so windows are never covered.
    pub panel_height: i32,
    /// Height of the dock.
    pub dock_height: i32,
    /// Size of a dock icon.
    pub dock_icon: i32,
    /// Height of a launcher row.
    pub launcher_row: i32,
    /// Width of a Finder-style sidebar.
    pub sidebar_width: i32,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            panel_height: 28,
            dock_height: 64,
            dock_icon: 48,
            launcher_row: 36,
            sidebar_width: 176,
        }
    }
}

/// The complete design system for one appearance.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignSystem {
    /// Human name (`Tahoe Light`, `Tahoe Dark`).
    pub name: &'static str,
    /// Whether this is the dark appearance.
    pub is_dark: bool,
    /// Desktop wallpaper gradient.
    pub wallpaper: Gradient,
    /// Panel (menu bar) material.
    pub panel: Rgba,
    /// Dock material.
    pub dock: Rgba,
    /// Window background for clients that ask for no decoration.
    pub window_background: Rgba,
    /// Primary text.
    pub text: Rgba,
    /// Secondary text.
    pub text_muted: Rgba,
    /// Selection and focus accent.
    pub accent: Rgba,
    /// Hairline separators.
    pub separator: Rgba,
    /// Spacing scale.
    pub spacing: Spacing,
    /// Corner radii.
    pub radii: Radii,
    /// Shell metrics.
    pub metrics: Metrics,
}

/// Wallpaper gradient stops shared by both appearances.
///
/// A Tahoe-style wallpaper: deep indigo at the top-left through a violet
/// midtone to a warm horizon, which reads well behind translucent chrome in
/// both light and dark modes.
const WALLPAPER_STOPS: [(f32, (u8, u8, u8)); 5] = [
    (0.00, (0x12, 0x1A, 0x3A)),
    (0.28, (0x2C, 0x2A, 0x6E)),
    (0.55, (0x5A, 0x3F, 0x9B)),
    (0.80, (0xB4, 0x63, 0x8C)),
    (1.00, (0xF0, 0xA8, 0x7A)),
];

fn wallpaper() -> Gradient {
    Gradient {
        stops: WALLPAPER_STOPS
            .iter()
            .map(|(at, (r, g, b))| Stop {
                at: *at,
                color: Rgba::rgb8(*r, *g, *b),
            })
            .collect(),
    }
}

impl DesignSystem {
    /// The light appearance.
    #[must_use]
    pub fn light() -> Self {
        Self {
            name: "Tahoe Light",
            is_dark: false,
            wallpaper: wallpaper(),
            panel: Rgba::rgb8(0xF2, 0xF2, 0xF7).with_alpha(0.72),
            dock: Rgba::rgb8(0xF7, 0xF7, 0xFA).with_alpha(0.68),
            window_background: Rgba::rgb8(0xFF, 0xFF, 0xFF),
            text: Rgba::rgb8(0x1C, 0x1C, 0x1E),
            text_muted: Rgba::rgb8(0x1C, 0x1C, 0x1E).with_alpha(0.55),
            accent: Rgba::rgb8(0x0A, 0x84, 0xFF),
            separator: Rgba::rgb8(0x3C, 0x3C, 0x43).with_alpha(0.18),
            spacing: Spacing::default(),
            radii: Radii::default(),
            metrics: Metrics::default(),
        }
    }

    /// The dark appearance.
    #[must_use]
    pub fn dark() -> Self {
        Self {
            name: "Tahoe Dark",
            is_dark: true,
            wallpaper: wallpaper(),
            panel: Rgba::rgb8(0x1C, 0x1C, 0x22).with_alpha(0.66),
            dock: Rgba::rgb8(0x2A, 0x2A, 0x33).with_alpha(0.62),
            window_background: Rgba::rgb8(0x1E, 0x1E, 0x24),
            text: Rgba::rgb8(0xF5, 0xF5, 0xF7),
            text_muted: Rgba::rgb8(0xF5, 0xF5, 0xF7).with_alpha(0.55),
            accent: Rgba::rgb8(0x0A, 0x84, 0xFF),
            separator: Rgba::rgb8(0xFF, 0xFF, 0xFF).with_alpha(0.12),
            spacing: Spacing::default(),
            radii: Radii::default(),
            metrics: Metrics::default(),
        }
    }

    /// Pick an appearance from a preference.
    #[must_use]
    pub fn from_preference(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    /// The appearance the desktop should start with.
    ///
    /// Honours `TAHOE_APPEARANCE=light|dark|auto`. `auto` always resolves to
    /// dark for now, because the Settings portal that would report the
    /// user's real preference is not implemented yet — this is documented
    /// rather than guessed at.
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var("TAHOE_APPEARANCE").as_deref() {
            Ok("light") => Self::light(),
            Ok("dark") => Self::dark(),
            _ => Self::dark(),
        }
    }

    /// Wallpaper colour at a normalised vertical position.
    #[must_use]
    pub fn wallpaper_at(&self, t: f32) -> Rgba {
        self.wallpaper.sample(t)
    }

    /// Replace the wallpaper sample with a solid colour, for low-power or
    /// headless rendering paths.
    #[must_use]
    pub fn flattened_wallpaper(&self) -> Rgba {
        let mid = self.wallpaper.sample(0.5);
        Rgba { a: 1.0, ..mid }
    }
}

impl Default for DesignSystem {
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_clamps_out_of_range_components() {
        let c = Rgba::new(2.0, -1.0, 0.5, 9.0);
        assert_eq!(c.r, 1.0);
        assert_eq!(c.g, 0.0);
        assert_eq!(c.a, 1.0);
    }

    #[test]
    fn rgb8_conversion_is_exact_at_bounds() {
        assert_eq!(Rgba::rgb8(0, 0, 0).r, 0.0);
        assert_eq!(Rgba::rgb8(255, 255, 255).r, 1.0);
        let mid = Rgba::rgb8(128, 0, 0);
        assert!((mid.r - 128.0 / 255.0).abs() < f32::EPSILON);
    }

    #[test]
    fn with_alpha_changes_only_alpha() {
        let c = Rgba::rgb8(10, 20, 30).with_alpha(0.25);
        assert_eq!(
            (c.r, c.g, c.b),
            (
                Rgba::rgb8(10, 20, 30).r,
                Rgba::rgb8(10, 20, 30).g,
                Rgba::rgb8(10, 20, 30).b
            )
        );
        assert_eq!(c.a, 0.25);
    }

    #[test]
    fn mix_endpoints_are_exact() {
        let a = Rgba::rgb8(0, 0, 0);
        let b = Rgba::rgb8(255, 255, 255);
        assert_eq!(a.mix(b, 0.0), a);
        assert_eq!(a.mix(b, 1.0), b);
        let mid = a.mix(b, 0.5);
        assert!((mid.r - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mix_clamps_t() {
        let a = Rgba::rgb8(0, 0, 0);
        let b = Rgba::rgb8(255, 255, 255);
        assert_eq!(a.mix(b, -5.0), a);
        assert_eq!(a.mix(b, 5.0), b);
    }

    #[test]
    fn gradient_with_no_stops_is_black() {
        let g = Gradient { stops: vec![] };
        assert_eq!(g.sample(0.5), Rgba::new(0.0, 0.0, 0.0, 1.0));
    }

    #[test]
    fn gradient_with_one_stop_is_constant() {
        let g = Gradient {
            stops: vec![Stop {
                at: 0.3,
                color: Rgba::rgb8(1, 2, 3),
            }],
        };
        assert_eq!(g.sample(0.0), Rgba::rgb8(1, 2, 3));
        assert_eq!(g.sample(1.0), Rgba::rgb8(1, 2, 3));
    }

    #[test]
    fn gradient_clamps_before_first_and_after_last_stop() {
        let g = Gradient {
            stops: vec![
                Stop {
                    at: 0.25,
                    color: Rgba::rgb8(10, 10, 10),
                },
                Stop {
                    at: 0.75,
                    color: Rgba::rgb8(20, 20, 20),
                },
            ],
        };
        assert_eq!(g.sample(0.0), Rgba::rgb8(10, 10, 10));
        assert_eq!(g.sample(1.0), Rgba::rgb8(20, 20, 20));
    }

    #[test]
    fn gradient_interpolates_between_stops() {
        let g = Gradient {
            stops: vec![
                Stop {
                    at: 0.0,
                    color: Rgba::rgb8(0, 0, 0),
                },
                Stop {
                    at: 1.0,
                    color: Rgba::rgb8(255, 255, 255),
                },
            ],
        };
        let mid = g.sample(0.5);
        assert!((mid.r - 0.5).abs() < 1e-6, "got {}", mid.r);
    }

    #[test]
    fn gradient_handles_duplicate_positions_without_dividing_by_zero() {
        let g = Gradient {
            stops: vec![
                Stop {
                    at: 0.5,
                    color: Rgba::rgb8(0, 0, 0),
                },
                Stop {
                    at: 0.5,
                    color: Rgba::rgb8(255, 255, 255),
                },
            ],
        };
        let c = g.sample(0.5);
        assert!(c.r.is_finite());
    }

    #[test]
    fn wallpaper_is_a_real_gradient() {
        let d = DesignSystem::dark();
        let top = d.wallpaper_at(0.0);
        let bottom = d.wallpaper_at(1.0);
        assert_ne!(top, bottom, "wallpaper must not be a flat colour");
        assert!(top.a == 1.0 && bottom.a == 1.0, "wallpaper must be opaque");
    }

    #[test]
    fn wallpaper_is_monotonic_in_brightness() {
        // The chosen stops go from dark indigo to a bright horizon, which
        // is what makes panel text legible in both appearances.
        let d = DesignSystem::light();
        let mut prev = -1.0f32;
        for i in 0..=10 {
            let c = d.wallpaper_at(i as f32 / 10.0);
            let lum = c.r + c.g + c.b;
            assert!(lum >= prev - 1e-5, "gradient darkens at {i}");
            prev = lum;
        }
    }

    #[test]
    fn both_appearances_share_metrics_but_differ_in_colour() {
        let l = DesignSystem::light();
        let d = DesignSystem::dark();
        assert_eq!(
            l.metrics, d.metrics,
            "layout must not shift between appearances"
        );
        assert_eq!(l.spacing, d.spacing);
        assert_eq!(l.radii, d.radii);
        assert_ne!(l.text, d.text);
        assert_eq!(l.wallpaper, d.wallpaper, "wallpaper is shared");
        assert!(!l.is_dark && d.is_dark);
    }

    #[test]
    fn panel_height_matches_reserved_compositor_area() {
        // The compositor reserves 28px for the panel; if this changes, the
        // compositor constant must change with it.
        assert_eq!(DesignSystem::dark().metrics.panel_height, 28);
    }

    #[test]
    fn accent_is_the_same_in_both_appearances() {
        assert_eq!(DesignSystem::light().accent, DesignSystem::dark().accent);
    }

    #[test]
    fn chrome_is_translucent_wallpaper_is_not() {
        let d = DesignSystem::dark();
        assert!(d.panel.a < 1.0, "panel should be a translucent material");
        assert!(d.dock.a < 1.0, "dock should be a translucent material");
        assert_eq!(d.wallpaper_at(0.5).a, 1.0);
    }

    #[test]
    fn flattened_wallpaper_is_opaque() {
        let d = DesignSystem::light();
        let flat = d.flattened_wallpaper();
        assert_eq!(flat.a, 1.0);
        assert_ne!(flat, Rgba::new(0.0, 0.0, 0.0, 1.0));
    }

    #[test]
    fn from_preference_selects_appearance() {
        assert!(DesignSystem::from_preference(true).is_dark);
        assert!(!DesignSystem::from_preference(false).is_dark);
    }

    #[test]
    fn spacing_scale_is_increasing() {
        let s = Spacing::default();
        assert!(s.xs < s.sm && s.sm < s.md && s.md < s.lg && s.lg < s.xl);
    }

    #[test]
    fn radii_are_ordered_and_non_negative() {
        let r = Radii::default();
        assert!(r.control > 0 && r.control < r.card && r.card < r.window && r.window < r.dock);
    }
}
