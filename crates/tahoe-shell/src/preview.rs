// SPDX-License-Identifier: MIT OR Apache-2.0
//! Software preview renderer for the Tahoe desktop.
//!
//! Composites the desktop chrome from [`DesignSystem`] tokens into a plain
//! RGB buffer, with no GPU and no Wayland. Two uses:
//!
//! 1. **Visual regression / design review** — `tahoe-preview` writes a PPM
//!    that shows exactly how the wallpaper, panel and dock will look,
//!    using the same gradient sampling and the same colours the compositor
//!    uploads as a texture.
//! 2. **Assertions** — the tests below check real properties of the output
//!    (the panel is darker than the wallpaper behind it, the dock is inset,
//!    a window is drawn opaque) instead of just "it did not crash".
//!
//! It is **not** a screenshot of the running compositor: it does not use
//! GL and does not contain client pixels. That distinction is deliberate
//! and is why the file is called "preview", not "screenshot".

use crate::design::{DesignSystem, Rgba};

/// An 8-bit RGB image, top row first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 3` bytes, RGB, top row first.
    pub pixels: Vec<u8>,
}

impl Image {
    /// A black image of the given size.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width as usize) * (height as usize) * 3],
        }
    }

    /// The RGB triple at `(x, y)`, or `None` when out of bounds.
    #[must_use]
    pub fn get(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = ((y as usize) * (self.width as usize) + (x as usize)) * 3;
        Some([self.pixels[i], self.pixels[i + 1], self.pixels[i + 2]])
    }

    /// Alpha-blend `colour` over the pixel at `(x, y)`, clipped to bounds.
    pub fn blend(&mut self, x: i32, y: i32, colour: Rgba) {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return;
        }
        let Some(existing) = self.get(x as u32, y as u32) else {
            return;
        };
        let a = colour.a;
        let mix = |dst: u8, src: f32| -> u8 {
            let dst = f32::from(dst) / 255.0;
            let out = src * a + dst * (1.0 - a);
            (out.clamp(0.0, 1.0) * 255.0).round() as u8
        };
        let i = ((y as usize) * (self.width as usize) + (x as usize)) * 3;
        self.pixels[i] = mix(existing[0], colour.r);
        self.pixels[i + 1] = mix(existing[1], colour.g);
        self.pixels[i + 2] = mix(existing[2], colour.b);
    }

    /// Fill a rectangle with a blended colour.
    pub fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, colour: Rgba) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.blend(xx, yy, colour);
            }
        }
    }

    /// Encode as a binary PPM (P6), which every image tool reads.
    #[must_use]
    pub fn to_ppm(&self) -> Vec<u8> {
        let mut out = format!("P6\n{} {}\n255\n", self.width, self.height).into_bytes();
        out.extend_from_slice(&self.pixels);
        out
    }
}

/// A rectangle the preview should draw as a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewWindow {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
    /// Whether the window is focused (draws with the accent edge).
    pub focused: bool,
}

impl PreviewWindow {
    /// A window at the given geometry.
    #[must_use]
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self {
            x,
            y,
            w,
            h,
            focused: false,
        }
    }

    /// Mark the window focused.
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
}

/// Composite the desktop chrome.
///
/// Order, bottom to top: wallpaper, panel, dock, windows. The panel and dock
/// are translucent materials blended over the wallpaper, which is why the
/// preview renderer exists as its own pass rather than a flat fill.
#[must_use]
pub fn render_desktop(
    design: &DesignSystem,
    width: u32,
    height: u32,
    windows: &[PreviewWindow],
) -> Image {
    let mut img = Image::new(width, height);

    // Wallpaper: sample the gradient per row, exactly as the GL path does
    // when it builds its texture.
    let last_row = height.saturating_sub(1).max(1);
    for y in 0..height {
        let t = y as f32 / last_row as f32;
        let colour = design.wallpaper_at(t);
        img.fill(0, y as i32, width as i32, 1, Rgba { a: 1.0, ..colour });
    }

    let metrics = design.metrics;

    // Panel: the strip the compositor reserves for the shell.
    img.fill(0, 0, width as i32, metrics.panel_height, design.panel);
    // Hairline under the panel, so it reads as a surface.
    img.fill(
        0,
        metrics.panel_height - 1,
        width as i32,
        1,
        design.separator,
    );

    // Dock: horizontally centred, floating above the bottom edge.
    let dock_w =
        (metrics.dock_icon * 6 + design.spacing.md * 7).min(width as i32 - design.spacing.xl * 2);
    if dock_w > 0 && height as i32 > metrics.dock_height + design.spacing.lg * 2 {
        let dock_x = (width as i32 - dock_w) / 2;
        let dock_y = height as i32 - metrics.dock_height - design.spacing.lg;
        img.fill(dock_x, dock_y, dock_w, metrics.dock_height, design.dock);
        img.fill(dock_x, dock_y, dock_w, 1, design.separator);
    }

    // Windows, bottom to top as given.
    for window in windows {
        img.fill(
            window.x,
            window.y,
            window.w,
            window.h,
            design.window_background,
        );
        // A 1px border: accent for the focused window, separator otherwise.
        let border = if window.focused {
            design.accent
        } else {
            design.separator
        };
        img.fill(window.x, window.y, window.w, 1, border);
        img.fill(window.x, window.y + window.h - 1, window.w, 1, border);
        img.fill(window.x, window.y, 1, window.h, border);
        img.fill(window.x + window.w - 1, window.y, 1, window.h, border);
    }

    img
}

#[cfg(test)]
mod tests {
    use super::*;

    fn design() -> DesignSystem {
        DesignSystem::dark()
    }

    #[test]
    fn image_starts_black_and_has_the_right_stride() {
        let img = Image::new(4, 3);
        assert_eq!(img.pixels.len(), 4 * 3 * 3);
        assert_eq!(img.get(0, 0), Some([0, 0, 0]));
        assert_eq!(img.get(4, 0), None);
        assert_eq!(img.get(0, 3), None);
    }

    #[test]
    fn blend_out_of_bounds_is_ignored_not_panicking() {
        let mut img = Image::new(2, 2);
        img.blend(-1, 0, Rgba::rgb8(255, 0, 0));
        img.blend(0, -1, Rgba::rgb8(255, 0, 0));
        img.blend(2, 0, Rgba::rgb8(255, 0, 0));
        img.blend(0, 2, Rgba::rgb8(255, 0, 0));
        assert_eq!(img.get(0, 0), Some([0, 0, 0]));
    }

    #[test]
    fn opaque_blend_replaces_the_pixel() {
        let mut img = Image::new(1, 1);
        img.blend(0, 0, Rgba::rgb8(10, 20, 30));
        assert_eq!(img.get(0, 0), Some([10, 20, 30]));
    }

    #[test]
    fn translucent_blend_lands_between_the_two_colours() {
        let mut img = Image::new(1, 1);
        img.blend(0, 0, Rgba::rgb8(0, 0, 0));
        img.blend(0, 0, Rgba::rgb8(255, 255, 255).with_alpha(0.5));
        let [r, _, _] = img.get(0, 0).unwrap();
        assert!((120..=135).contains(&r), "expected roughly half, got {r}");
    }

    #[test]
    fn ppm_header_is_well_formed() {
        let img = Image::new(3, 2);
        let ppm = img.to_ppm();
        assert!(ppm.starts_with(b"P6\n3 2\n255\n"));
        assert_eq!(ppm.len(), "P6\n3 2\n255\n".len() + 3 * 2 * 3);
    }

    #[test]
    fn desktop_preview_has_the_requested_size() {
        let img = render_desktop(&design(), 320, 200, &[]);
        assert_eq!((img.width, img.height), (320, 200));
    }

    #[test]
    fn wallpaper_actually_varies_down_the_image() {
        let img = render_desktop(&design(), 64, 128, &[]);
        let top = img.get(32, 1).unwrap();
        let bottom = img.get(32, 127).unwrap();
        assert_ne!(top, bottom, "wallpaper must be a gradient, not a flat fill");
    }

    #[test]
    fn panel_covers_the_reserved_strip_only() {
        let d = design();
        let img = render_desktop(&d, 200, 300, &[]);
        // Inside the panel: the panel material, not the raw wallpaper.
        let in_panel = img.get(100, 4).unwrap();
        let just_below = img.get(100, d.metrics.panel_height as u32).unwrap();
        assert_ne!(in_panel, just_below);

        // The separating hairline sits on the last panel row.
        let hairline = img.get(100, (d.metrics.panel_height - 1) as u32).unwrap();
        assert_ne!(
            hairline, in_panel,
            "panel must be delineated from the work area"
        );
    }

    #[test]
    fn panel_material_darkens_the_dark_wallpaper() {
        let d = design();
        // For the dark appearance the panel is a dark translucent material,
        // so the strip must end up darker than the wallpaper beneath it.
        let img = render_desktop(&d, 100, 400, &[]);
        let panel = img.get(50, 5).unwrap();
        let work = img.get(50, (d.metrics.panel_height + 5) as u32).unwrap();
        let lum = |p: [u8; 3]| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]);
        assert!(
            lum(panel) < lum(work),
            "panel {panel:?} should be darker than the work area {work:?}"
        );
    }

    #[test]
    fn dock_is_drawn_and_horizontally_centred() {
        let d = design();
        let img = render_desktop(&d, 800, 600, &[]);
        let dock_y = 600 - d.metrics.dock_height - d.spacing.lg;
        let centre = img
            .get(400, (dock_y + d.metrics.dock_height / 2) as u32)
            .unwrap();
        let near_edge = img
            .get(5, (dock_y + d.metrics.dock_height / 2) as u32)
            .unwrap();
        assert_ne!(
            centre, near_edge,
            "a dock should occupy the horizontal centre"
        );
    }

    #[test]
    fn windows_are_drawn_opaque_and_bordered() {
        let d = design();
        let win = PreviewWindow::new(40, 60, 200, 120).focused(true);
        let img = render_desktop(&d, 400, 300, &[win]);

        let inside = img.get(140, 120).unwrap();
        let expected = [
            (d.window_background.r * 255.0).round() as u8,
            (d.window_background.g * 255.0).round() as u8,
            (d.window_background.b * 255.0).round() as u8,
        ];
        assert_eq!(
            inside, expected,
            "window interior must be the window background"
        );

        // The focused border uses the accent colour.
        let border = img.get(140, 60).unwrap();
        let accent = [
            (d.accent.r * 255.0).round() as u8,
            (d.accent.g * 255.0).round() as u8,
            (d.accent.b * 255.0).round() as u8,
        ];
        assert_eq!(
            border, accent,
            "focused window must be outlined with the accent"
        );
    }

    #[test]
    fn unfocused_windows_do_not_use_the_accent() {
        let d = design();
        let win = PreviewWindow::new(40, 60, 200, 120);
        let img = render_desktop(&d, 400, 300, &[win]);
        let border = img.get(140, 60).unwrap();
        let accent = [
            (d.accent.r * 255.0).round() as u8,
            (d.accent.g * 255.0).round() as u8,
            (d.accent.b * 255.0).round() as u8,
        ];
        assert_ne!(
            border, accent,
            "an unfocused window must not steal the accent"
        );
    }

    #[test]
    fn windows_outside_the_canvas_do_not_panic() {
        let win = PreviewWindow::new(-50, -50, 40, 40);
        let img = render_desktop(&design(), 100, 100, &[win]);
        assert_eq!((img.width, img.height), (100, 100));
        let win2 = PreviewWindow::new(90, 90, 500, 500);
        let img2 = render_desktop(&design(), 100, 100, &[win2]);
        assert!(img2.get(99, 99).is_some());
    }

    #[test]
    fn tiny_and_degenerate_canvases_do_not_panic() {
        for (w, h) in [(0u32, 0u32), (1, 1), (1, 200), (200, 1), (2, 3)] {
            let img = render_desktop(&design(), w, h, &[PreviewWindow::new(0, 0, 10, 10)]);
            assert_eq!(img.pixels.len(), (w as usize) * (h as usize) * 3);
        }
    }

    #[test]
    fn light_and_dark_produce_different_chrome() {
        let light = render_desktop(&DesignSystem::light(), 100, 100, &[]);
        let dark = render_desktop(&DesignSystem::dark(), 100, 100, &[]);
        assert_ne!(
            light.get(50, 5),
            dark.get(50, 5),
            "panel colours must differ"
        );
        // Both share the wallpaper, so the lower area matches.
        assert_eq!(light.get(50, 95), dark.get(50, 95));
    }
}
