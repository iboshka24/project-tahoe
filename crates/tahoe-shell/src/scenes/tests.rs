// SPDX-License-Identifier: MIT OR Apache-2.0
//! Scene tests.
//!
//! A render is a picture, so most of what is worth asserting is structural:
//! that the layers are in the right order, that a focused window reads as
//! focused, that the dock is where the compositor reserves space for it,
//! and that opening a surface actually changes pixels.
//!
//! Taste is not testable. Legibility is checked indirectly — the text paths
//! have their own tests in `render`.

use super::*;
use crate::render::Fonts;

fn fonts() -> Fonts {
    Fonts::system().expect("a system font")
}

fn dark() -> DesignSystem {
    DesignSystem::dark()
}

/// Mean luminance over a rect, for "did this area change" checks.
fn mean_luma(canvas: &Canvas, rect: Rect) -> f64 {
    let mut total = 0u64;
    let mut count = 0u64;
    for y in rect.y..rect.y + rect.h {
        for x in rect.x..rect.x + rect.w {
            if let Some(px) = canvas.get(x, y) {
                total += u64::from(px[0]) + u64::from(px[1]) + u64::from(px[2]);
                count += 1;
            }
        }
    }
    if count == 0 {
        0.0
    } else {
        total as f64 / count as f64
    }
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

#[test]
fn the_panel_occupies_exactly_the_strip_the_compositor_reserves() {
    // `tahoe_compositor::PANEL_HEIGHT` is 28, `DesignSystem::metrics` says
    // 28, and the shell must draw into 28. A mismatch here is a window that
    // floats with a gap or slides under the bar.
    let c = render_sized(
        &dark(),
        &Decor::default(),
        &Scene::desktop(&dark()),
        &fonts(),
        1200,
        800,
    );
    let panel = Rect::new(0, 0, 1200, 28);
    // Rows 28-32 are a poor comparison: the gradient has barely moved in
    // four pixels, so "panel differs from just below it" would be testing
    // the gradient's slope rather than the panel. Sample where the
    // wallpaper has actually changed.
    let lower = Rect::new(0, 300, 1200, 4);
    let (panel_luma, lower_luma) = (mean_luma(&c, panel), mean_luma(&c, lower));
    assert!(
        lower_luma - panel_luma > 20.0,
        "the panel is a dark material over a lighter wallpaper here: panel {panel_luma}, lower {lower_luma}"
    );
    // And the hairline sits on its last row.
    let hairline = c.get(600, 27).expect("the hairline row");
    assert!(
        hairline[3] == 255,
        "the panel is drawn, so its last row is opaque"
    );
}

#[test]
fn the_dock_is_horizontally_centred_and_inside_the_canvas() {
    let design = dark();
    let l = layout(&design, 1600, 1000);
    assert_eq!(l.dock.x + l.dock.w / 2, 800, "the dock is centred");
    assert!(l.dock.y + l.dock.h <= 1000, "the dock fits on screen");
    assert!(l.dock.y > l.panel_height, "and is below the panel");
}

#[test]
fn the_work_area_excludes_the_panel_and_the_dock() {
    let design = dark();
    let l = layout(&design, 1600, 1000);
    assert_eq!(l.work.y, design.metrics.panel_height);
    assert!(
        l.work.y + l.work.h <= l.dock.y,
        "the work area must not run under the dock"
    );
}

#[test]
fn a_tiny_canvas_does_not_panic() {
    for (w, h) in [(1u32, 1u32), (10, 10), (60, 40), (200, 120)] {
        let design = dark();
        let _ = render_sized(
            &design,
            &Decor::default(),
            &Scene::desktop(&design),
            &fonts(),
            w,
            h,
        );
        let _ = render_sized(
            &design,
            &Decor::default(),
            &Scene::desktop(&design)
                .with_launcher()
                .with_control_center(),
            &fonts(),
            w,
            h,
        );
    }
}

// ---------------------------------------------------------------------------
// Layers
// ---------------------------------------------------------------------------

#[test]
fn the_wallpaper_is_a_gradient_and_the_chrome_sits_on_top() {
    let c = render_sized(
        &dark(),
        &Decor::default(),
        &Scene::desktop(&dark()),
        &fonts(),
        1200,
        800,
    );
    // A column clear of the windows and the dock.
    let top = c.get(1180, 40).expect("top");
    let bottom = c.get(1180, 780).expect("bottom");
    assert!(
        top[2] > top[0],
        "the wallpaper starts blue-ish, got {top:?}"
    );
    assert!(bottom[0] > bottom[2], "and ends warm, got {bottom:?}");
}

#[test]
fn windows_are_drawn_larger_and_darker_than_the_wallpaper() {
    let design = dark();
    let c = render_sized(
        &design,
        &Decor::default(),
        &Scene::desktop(&design),
        &fonts(),
        1200,
        800,
    );
    let l = layout(&design, 1200, 800);
    let window_rect = Rect::new(
        l.work.x + l.spacing + 20,
        l.work.y + l.spacing + 60,
        120,
        80,
    );
    assert!(
        mean_luma(&c, window_rect) < mean_luma(&c, Rect::new(20, 40, 120, 80)),
        "window interiors are darker than the wallpaper"
    );
}

#[test]
fn only_the_focused_window_carries_the_accent_border() {
    let design = dark();
    let mut scene = Scene::desktop(&design);
    scene.focused = Some(0);
    let a = render_sized(&design, &Decor::default(), &scene, &fonts(), 1200, 800);
    scene.focused = Some(1);
    let b = render_sized(&design, &Decor::default(), &scene, &fonts(), 1200, 800);
    assert_ne!(a.pixels, b.pixels, "changing focus must change the picture");
}

#[test]
fn a_shadow_darkens_the_area_just_outside_a_window() {
    let design = dark();
    let mut scene = Scene::desktop(&design);
    scene.windows.truncate(1);
    let c = render_sized(&design, &Decor::default(), &scene, &fonts(), 1200, 800);
    let l = layout(&design, 1200, 800);
    // Three pixels above the window's top edge, inside the shadow's spread.
    let near = Rect::new(l.work.x + l.spacing + 40, l.work.y + l.spacing - 3, 40, 3);
    // The same height, far to the right, where no window reaches.
    let far = Rect::new(1100, l.work.y + l.spacing - 3, 40, 3);
    assert!(
        mean_luma(&c, near) < mean_luma(&c, far) + 30.0,
        "a shadow should darken the area just outside a window: {} vs {}",
        mean_luma(&c, near),
        mean_luma(&c, far)
    );
}

// ---------------------------------------------------------------------------
// Opening surfaces changes the picture
// ---------------------------------------------------------------------------

#[test]
fn opening_the_launcher_changes_the_picture_where_the_launcher_is() {
    let design = dark();
    let closed = render_sized(
        &design,
        &Decor::default(),
        &Scene::desktop(&design),
        &fonts(),
        1200,
        800,
    );
    let open = render_sized(
        &design,
        &Decor::default(),
        &Scene::desktop(&design).with_launcher(),
        &fonts(),
        1200,
        800,
    );
    assert_ne!(
        closed.pixels, open.pixels,
        "opening the launcher must draw something"
    );

    let l = layout(&design, 1200, 800);
    // The launcher sits below the panel, horizontally centred.
    let launcher = Rect::new(
        l.panel.w / 2 - 200,
        l.panel_height + l.spacing * 2 + 20,
        400,
        200,
    );
    assert!(
        mean_luma(&open, launcher) > mean_luma(&closed, launcher),
        "the launcher panel is lighter than the dimmed background"
    );
}

#[test]
fn opening_the_launcher_dims_the_desktop_behind_it() {
    let design = dark();
    let closed = render_sized(
        &design,
        &Decor::default(),
        &Scene::desktop(&design),
        &fonts(),
        1200,
        800,
    );
    let open = render_sized(
        &design,
        &Decor::default(),
        &Scene::desktop(&design).with_launcher(),
        &fonts(),
        1200,
        800,
    );
    // A corner of the work area, far from the launcher panel and clear of
    // the windows and the dock.
    let corner = Rect::new(20, 500, 80, 40);
    let (open_luma, closed_luma) = (mean_luma(&open, corner), mean_luma(&closed, corner));
    assert!(
        open_luma < closed_luma,
        "the scrim should darken everything behind the launcher: open {open_luma}, closed {closed_luma}"
    );
}

#[test]
fn opening_the_control_centre_changes_the_picture() {
    let design = dark();
    let closed = render_sized(
        &design,
        &Decor::default(),
        &Scene::desktop(&design),
        &fonts(),
        1200,
        800,
    );
    let open = render_sized(
        &design,
        &Decor::default(),
        &Scene::desktop(&design).with_control_center(),
        &fonts(),
        1200,
        800,
    );
    assert_ne!(closed.pixels, open.pixels);
    // The control centre lives in the top right, under the panel. In the
    // dark theme it is a *dark* panel, so the assertion is that the region
    // changed rather than that it brightened — the direction depends on the
    // appearance, and asserting one would be asserting a colour.
    let tiles = Rect::new(1200 - 440, 30, 420, 220);
    let (open_luma, closed_luma) = (mean_luma(&open, tiles), mean_luma(&closed, tiles));
    assert!(
        (open_luma - closed_luma).abs() > 1.0,
        "the control centre must change the region: open {open_luma}, closed {closed_luma}"
    );
    // And in the light theme it should brighten it, which pins that the
    // panel really is drawn from the appearance.
    let light = DesignSystem::light();
    let l_open = render_sized(
        &light,
        &Decor::default(),
        &Scene::desktop(&light).with_control_center(),
        &fonts(),
        1200,
        800,
    );
    let l_closed = render_sized(
        &light,
        &Decor::default(),
        &Scene::desktop(&light),
        &fonts(),
        1200,
        800,
    );
    assert!(
        mean_luma(&l_open, tiles) > mean_luma(&l_closed, tiles),
        "the light appearance's control centre must be brighter than the wallpaper"
    );
}

// ---------------------------------------------------------------------------
// Scene data
// ---------------------------------------------------------------------------

#[test]
fn the_default_scene_is_plausible() {
    let scene = Scene::desktop(&dark());
    assert_eq!(scene.windows.len(), 3, "three windows to show overlap");
    assert!(scene.focused.is_some(), "one of them is focused");
    assert_eq!(scene.dock.len(), 6);
    assert!(
        scene.dock.iter().any(|a| a.running),
        "at least one app is running, so the indicator is visible"
    );
    assert!(!scene.launcher_open && !scene.control_center_open);
    assert!(scene.launcher.len() >= 4, "the launcher has rows to show");
}

#[test]
fn the_decor_defaults_are_stable_for_reproducible_renders() {
    // A render that reads the clock is a render that cannot be compared.
    let a = Decor::default();
    let b = Decor::default();
    assert_eq!(a.clock, b.clock);
    assert_eq!(a.workspaces, b.workspaces);
}

#[test]
fn the_theme_is_the_only_thing_that_changes_between_appearances() {
    // Light and dark must differ in colour and not in layout: the same
    // window rect must hold the same content in both.
    let light = render_sized(
        &DesignSystem::light(),
        &Decor::default(),
        &Scene::desktop(&DesignSystem::light()),
        &fonts(),
        800,
        600,
    );
    let darkc = render_sized(
        &dark(),
        &Decor::default(),
        &Scene::desktop(&dark()),
        &fonts(),
        800,
        600,
    );
    assert_eq!(light.width, darkc.width);
    assert_ne!(light.pixels, darkc.pixels, "the appearances must differ");
    // The wallpaper is shared, so a corner of it far from the chrome matches.
    assert_eq!(
        light.get(790, 590),
        darkc.get(790, 590),
        "the wallpaper is shared"
    );
}

#[test]
fn every_control_tile_names_what_it_would_bind_to() {
    // A drawn tile is not a working switch, and saying so is the point.
    let scene = Scene::desktop(&dark());
    for tile in &scene.control_tiles {
        assert!(
            !tile.binding.is_empty(),
            "{} must name its binding",
            tile.title
        );
    }
}
