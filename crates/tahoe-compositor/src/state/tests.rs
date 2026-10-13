// SPDX-License-Identifier: MIT OR Apache-2.0
//! State-level tests.
//!
//! These exercise the parts of [`Tahoe`] that need a real Smithay display
//! but no backend at all: global registration, geometry policy, counters and
//! the human-readable description. Protocol behaviour driven by an actual
//! client lives in `tests/wayland_client.rs`.

use smithay::reexports::wayland_server::Display;
use smithay::utils::Size;

use super::*;

fn state(width: i32, height: i32) -> (Display<Tahoe>, Tahoe) {
    let display = Display::<Tahoe>::new().expect("display");
    let handle = display.handle();
    let state = Tahoe::new(&handle, Size::from((width, height)));
    // Keep the display alive: dropping it would kill every global.
    (display, state)
}

#[test]
fn constructs_with_the_requested_output_size() {
    let (_d, state) = state(1280, 720);
    assert_eq!(state.output_size, Size::from((1280, 720)));
}

#[test]
fn panel_is_reserved_and_work_area_shrinks() {
    let (_d, state) = state(1000, 600);
    let area = work_area(state.output_size);
    assert_eq!(area.x, 0);
    assert_eq!(area.y, PANEL_HEIGHT);
    assert_eq!(area.w, 1000);
    assert_eq!(area.h, 600 - PANEL_HEIGHT);
}

#[test]
fn work_area_never_collapses_on_a_tiny_output() {
    let area = work_area(Size::from((100, 10)));
    assert!(
        area.h >= 1,
        "work area must stay positive or nothing can be placed"
    );
}

#[test]
fn suggested_size_fits_inside_the_work_area() {
    let (_d, state) = state(1000, 700);
    let s = state.suggested_size();
    let area = work_area(state.output_size);
    assert!(s.w <= area.w && s.h <= area.h, "{s:?} must fit in {area:?}");
    assert!(s.w > 0 && s.h > 0);
}

#[test]
fn suggested_size_respects_the_minimum_floor_on_a_modest_output() {
    // 400px of work area is above the 320px comfortable minimum, so the
    // window takes the minimum rather than a sliver of 60%.
    let (_d, state) = state(400, 300);
    let s = state.suggested_size();
    assert_eq!(s.w, 320);
    let area = work_area(state.output_size);
    assert!(s.w <= area.w);
    assert!(s.h <= area.h);
}

#[test]
fn suggested_size_clamps_to_the_output_not_beyond() {
    let (_d, state) = state(200, 160);
    let s = state.suggested_size();
    assert!(s.w <= 200, "got {}", s.w);
    assert!(s.h <= 160, "got {}", s.h);
}

#[test]
fn suggested_size_never_panics_for_any_output_size() {
    // Regression: `clamp(320, area.w)` aborts when the output is narrower
    // than 320px, which a small window or a VM console easily is.
    for w in [1, 2, 16, 64, 200, 319, 320, 321, 1000, 3840] {
        for h in [1, 2, 16, 64, 200, 239, 240, 241, 700, 2160] {
            let (_d, state) = state(w, h);
            let s = state.suggested_size();
            assert!(s.w >= 1 && s.h >= 1, "{w}x{h} produced {s:?}");
            let area = work_area(state.output_size);
            assert!(
                s.w <= area.w.max(1),
                "{w}x{h} produced {s:?} wider than {area:?}"
            );
            assert!(
                s.h <= area.h.max(1),
                "{w}x{h} produced {s:?} taller than {area:?}"
            );
        }
    }
}

#[test]
fn preferred_dimension_orders_its_bounds() {
    // Below the minimum: use everything available.
    assert_eq!(preferred_dimension(100, 320), 100);
    assert_eq!(preferred_dimension(1, 320), 1);
    // Exactly the minimum: still inside bounds.
    assert_eq!(preferred_dimension(320, 320), 320);
    // Above it: a comfortable fraction, but never below the minimum.
    assert_eq!(preferred_dimension(1000, 320), 600);
    assert_eq!(preferred_dimension(400, 320), 320);
    // Degenerate input cannot produce a zero or negative dimension.
    assert_eq!(preferred_dimension(0, 320), 1);
    assert_eq!(preferred_dimension(-50, 320), 1);
}

#[test]
fn a_fresh_compositor_has_no_clients_and_no_windows() {
    let (_d, state) = state(800, 600);
    assert_eq!(state.clients_connected(), 0);
    assert_eq!(state.clients_disconnected(), 0);
    assert!(!state.saw_full_client_lifecycle());
    assert_eq!(state.windows.len(), 0);
    assert_eq!(state.layer_surface_count(), 0);
    assert_eq!(state.focused_surface(), None);
}

#[test]
fn counters_start_at_zero_and_only_move_when_counted() {
    let (_d, state) = state(800, 600);
    assert_eq!(state.stats, CompositorStats::default());
    state.count_client_connected();
    assert_eq!(state.clients_connected(), 1);
    assert_eq!(state.clients_disconnected(), 0);
    assert!(!state.saw_full_client_lifecycle());
    state.client_counters().count_disconnected();
    assert!(state.saw_full_client_lifecycle());
}

#[test]
fn frame_callbacks_on_an_empty_compositor_send_nothing() {
    let (_d, mut state) = state(800, 600);
    assert_eq!(state.send_frame_callbacks(), 0);
    assert_eq!(state.stats.frames_sent, 0);
}

#[test]
fn describe_reports_geometry_and_state() {
    let (_d, state) = state(1440, 900);
    let text = state.describe();
    assert!(text.contains("output=1440x900"), "{text}");
    assert!(text.contains("panel_height=28"), "{text}");
    assert!(text.contains("windows=0"), "{text}");
    assert!(text.contains("work_area="), "{text}");
}

#[test]
fn describe_lists_layer_surfaces_once_present() {
    let (_d, mut state) = state(900, 700);
    // Simulate what the layer-shell handler records, without a client.
    let before = state.describe();
    assert!(
        before.contains("layer_surfaces=") || before.contains("stats="),
        "{before}"
    );
    state.stats.layer_surfaces_created += 1;
    assert!(state.describe().contains("layer_surfaces_created: 1"));
}

#[test]
fn pointer_motion_updates_location() {
    let (_d, mut state) = state(800, 600);
    state.on_pointer_motion(120.0, 340.0);
    assert!((state.pointer_location.x - 120.0).abs() < f64::EPSILON);
    assert!((state.pointer_location.y - 340.0).abs() < f64::EPSILON);
}

#[test]
fn pointer_button_with_no_windows_does_not_panic() {
    let (_d, mut state) = state(800, 600);
    state.on_pointer_motion(10.0, 10.0);
    state.on_pointer_button(0x110, smithay::backend::input::ButtonState::Pressed, false);
    state.on_pointer_button(0x110, smithay::backend::input::ButtonState::Released, false);
    assert_eq!(state.stats.buttons, 2);
    assert_eq!(state.stats.moves, 0, "no window to move");
}

#[test]
fn super_drag_with_no_window_starts_no_move() {
    let (_d, mut state) = state(800, 600);
    state.on_pointer_motion(50.0, 50.0);
    state.on_pointer_button(0x110, smithay::backend::input::ButtonState::Pressed, true);
    assert_eq!(state.stats.moves, 0);
    assert!(state.windows.drag().is_none());
}

#[test]
fn keyboard_events_are_counted_even_with_no_focus() {
    let (_d, mut state) = state(800, 600);
    state.on_keyboard_key(30u32.into(), smithay::backend::input::KeyState::Pressed);
    state.on_keyboard_key(30u32.into(), smithay::backend::input::KeyState::Released);
    assert_eq!(state.stats.keys, 2);
}

#[test]
fn window_rectangle_matches_the_managed_rect() {
    let r = window_rectangle(Rect::new(10, 20, 300, 200));
    assert_eq!(r.loc, (10, 20).into());
    assert_eq!(r.size, (300, 200).into());
}

#[test]
fn now_ms_is_monotonic_ish() {
    let (_d, state) = state(800, 600);
    let a = state.now_ms();
    let b = state.now_ms();
    assert!(b >= a);
}

#[test]
fn dropping_state_releases_windows_cleanly() {
    let (_d, mut state) = state(800, 600);
    let area = work_area(state.output_size);
    state
        .windows
        .insert(area, (100, 100), crate::Cascade::default());
    assert_eq!(state.windows.len(), 1);
    drop(state);
}
