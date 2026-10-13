// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-compositor`: the Tahoe Wayland compositor.
//!
//! # Structure
//!
//! - [`wm`] — window policy (z-order, focus, hit testing, dragging). No
//!   Wayland types, so it is unit-tested without a socket.
//! - [`state`] — Smithay protocol handlers, a thin adapter over [`wm`].
//! - [`backend`] — how frames get to a screen. `headless` serves clients
//!   with no display at all (used by the test suite and by CI);
//!   `winit` opens a nested window for development on a running desktop.
//!
//! The split is deliberate: the parts that are hard to test (GPU, display
//! server, seats) are isolated behind a small surface, and everything
//! interesting is testable with `cargo test`.

pub mod backend;
pub mod state;
pub mod wm;

pub use state::{ClientState, CompositorStats, PANEL_HEIGHT, Tahoe, work_area};
pub use wm::{Cascade, ManagedWindow, Rect, WindowId, WindowStack};

/// Protocol surface this compositor advertises.
///
/// This list is documentation *and* a test fixture: `tests/protocols.rs`
/// asserts that a real client sees exactly these globals, so the list can
/// never silently drift away from what is actually implemented.
///
/// Not listed here means not implemented — see `docs/limitations.md`.
pub const ADVERTISED_GLOBALS: &[&str] = &[
    "wl_compositor",
    "wl_shm",
    "wl_seat",
    "wl_output",
    "wl_data_device_manager",
    "xdg_wm_base",
    "zwlr_layer_shell_v1",
];
