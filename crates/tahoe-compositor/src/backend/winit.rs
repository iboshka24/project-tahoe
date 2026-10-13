// SPDX-License-Identifier: MIT OR Apache-2.0
//! Windowed development backend.
//!
//! Runs the compositor nested inside an existing Wayland or X11 session, so
//! it can be developed and demonstrated without taking over a VT. It
//! renders a real desktop: the Tahoe wallpaper, the reserved panel strip,
//! then every client window in z-order.
//!
//! This backend exists for development. The shipping configuration is the
//! DRM/KMS backend (`--features tty`), which is not wired up yet — see
//! `docs/limitations.md`.

use std::time::Duration;

use smithay::{
    backend::{
        allocator::Fourcc,
        input::{AbsolutePositionEvent, InputEvent, KeyboardKeyEvent, PointerButtonEvent},
        renderer::{
            Color32F, Frame as _, Renderer as _,
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                solid::{SolidColorBuffer, SolidColorRenderElement},
                surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            },
            gles::GlesRenderer,
            utils::draw_render_elements,
        },
        winit::{self, WinitEvent},
    },
    reexports::{wayland_server::Display, winit::platform::pump_events::PumpStatus},
    utils::{Physical, Point, Rectangle, Scale, Size, Transform},
};
use tahoe_shell::design::DesignSystem;

use crate::{
    backend::SocketSpec,
    state::{PANEL_HEIGHT, Tahoe, work_area},
    wm::Rect,
};

/// A wallpaper bitmap at a fixed size, rebuilt only when that size changes.
struct Wallpaper {
    buffer: MemoryRenderBuffer,
    size: Size<i32, Physical>,
}

impl Wallpaper {
    /// Render the design system's wallpaper gradient into an RGBA bitmap.
    ///
    /// This is a real render, not a solid fill: each row samples the
    /// gradient, so the result matches [`DesignSystem::wallpaper_at`].
    fn render(design: &DesignSystem, size: Size<i32, Physical>) -> Self {
        let w = size.w.max(1) as usize;
        let h = size.h.max(1) as usize;
        let mut pixels = vec![0u8; w * h * 4];
        for y in 0..h {
            let t = if h <= 1 {
                0.0
            } else {
                y as f32 / (h - 1) as f32
            };
            let color = design.wallpaper_at(t);
            let (r, g, b, a) = (
                (color.r * 255.0).round() as u8,
                (color.g * 255.0).round() as u8,
                (color.b * 255.0).round() as u8,
                (color.a * 255.0).round() as u8,
            );
            for x in 0..w {
                let i = (y * w + x) * 4;
                // Abgr8888 in DRM naming is 0xAABBGGRR, which is byte order
                // R, G, B, A in memory — exactly what we want.
                pixels[i] = r;
                pixels[i + 1] = g;
                pixels[i + 2] = b;
                pixels[i + 3] = a;
            }
        }
        let buffer = MemoryRenderBuffer::from_slice(
            &pixels,
            Fourcc::Abgr8888,
            (size.w, size.h),
            1,
            Transform::Normal,
            None,
        );
        Self { buffer, size }
    }

    fn element(
        &self,
        renderer: &mut GlesRenderer,
    ) -> MemoryRenderBufferRenderElement<GlesRenderer> {
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            Point::<f64, Physical>::from((0.0, 0.0)),
            &self.buffer,
            None,
            None,
            None,
            Kind::Unspecified,
        )
        .expect("wallpaper buffer is valid")
    }

    /// True when the cached bitmap no longer matches the target size.
    fn needs_resize(&self, size: Size<i32, Physical>) -> bool {
        self.size != size
    }
}

/// How the windowed backend should behave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Exit automatically after this long, for automated smoke tests.
    pub exit_after: Option<Duration>,
    /// Draw the reserved panel strip. Off makes it visually obvious which
    /// part of the screen the shell will own.
    pub draw_panel: bool,
    /// Use the light appearance instead of the environment default.
    pub force_light: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            exit_after: None,
            draw_panel: true,
            force_light: false,
        }
    }
}

/// Run the compositor in a nested window until the user closes it (or
/// `exit_after` elapses).
///
/// # Errors
///
/// Returns an error when the winit backend or the Wayland display cannot be
/// initialised — for example when no graphical session is reachable.
pub fn run(socket: SocketSpec, options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let mut display: Display<Tahoe> = Display::new()?;
    let (mut backend, mut winit) = winit::init::<GlesRenderer>()?;
    let output_size = backend.window_size().to_logical(1);

    let mut state = Tahoe::new(&display.handle(), output_size);
    let listener = socket.bind().map_err(std::io::Error::other)?;

    let design = if options.force_light {
        DesignSystem::light()
    } else {
        DesignSystem::from_env()
    };
    let mut wallpaper = Wallpaper::render(&design, backend.window_size());

    // Advertise our own socket to children so a demo client connects here.
    if let SocketSpec::Named(name) = &socket {
        // SAFETY: set at startup, before any thread reads the environment.
        unsafe { std::env::set_var("WAYLAND_DISPLAY", name) };
    }
    let panel_color = Color32F::new(
        design.panel.r,
        design.panel.g,
        design.panel.b,
        design.panel.a,
    );
    let skip_panel = !options.draw_panel;

    tracing::info!(
        ?output_size,
        appearance = design.name,
        "tahoe compositor started"
    );

    let deadline = options.exit_after.map(|d| std::time::Instant::now() + d);

    loop {
        let status = winit.dispatch_new_events(|event| match event {
            WinitEvent::Resized { size, .. } => {
                tracing::debug!(?size, "resized");
            }
            WinitEvent::CloseRequested => {}
            WinitEvent::Input(event) => match event {
                InputEvent::Keyboard { event } => {
                    state.on_keyboard_key(event.key_code(), event.state());
                }
                InputEvent::PointerMotionAbsolute { event } => {
                    let pos = event.position();
                    state.on_pointer_motion(pos.x, pos.y);
                }
                InputEvent::PointerButton { event } => {
                    let (button, button_state) = (event.button_code(), event.state());
                    state.on_pointer_button(button, button_state, false);
                }
                _ => {}
            },
            _ => {}
        });

        match status {
            PumpStatus::Continue => {}
            PumpStatus::Exit(_) => return Ok(()),
        }
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            tracing::info!("exit_after elapsed, shutting down");
            return Ok(());
        }

        let physical = backend.window_size();
        let scale = Scale::from(1.0);
        let damage = Rectangle::from_size(physical);
        {
            let (renderer, mut framebuffer) = backend.bind()?;
            if wallpaper.needs_resize(physical) {
                wallpaper = Wallpaper::render(&design, physical);
            }
            let logical = physical.to_logical(1);
            let to_physical = |x: i32, y: i32| {
                Point::<i32, Physical>::from((
                    (f64::from(x) * scale.x).round() as i32,
                    (f64::from(y) * scale.y).round() as i32,
                ))
            };

            // Smithay's `render()` borrows the renderer for the frame's
            // lifetime, so every element must be built first.
            // Painter's algorithm, one batch per layer. Separate batches
            // keep each element slice homogeneous, which avoids an enum
            // wrapper around three unrelated element types. Every element
            // is built before the frame borrows the renderer.
            let wallpaper_batch = vec![wallpaper.element(renderer)];
            let panel_batch: Vec<SolidColorRenderElement> = if skip_panel {
                Vec::new()
            } else {
                let buffer = SolidColorBuffer::new((logical.w, PANEL_HEIGHT), panel_color);
                vec![SolidColorRenderElement::from_buffer(
                    &buffer,
                    Point::<i32, Physical>::from((0, 0)),
                    scale.x,
                    design.panel.a,
                    Kind::Unspecified,
                )]
            };
            let mut window_batch: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = Vec::new();
            for window in state.windows.windows() {
                let Some(toplevel) = state.toplevel_for(window.id) else {
                    continue;
                };
                window_batch.extend(render_elements_from_surface_tree(
                    renderer,
                    toplevel.wl_surface(),
                    to_physical(window.rect.x, window.rect.y),
                    scale.x,
                    1.0,
                    Kind::Unspecified,
                ));
            }

            let mut frame = renderer.render(&mut framebuffer, physical, Transform::Flipped180)?;
            frame.clear(Color32F::new(0.0, 0.0, 0.0, 1.0), &[damage])?;

            draw_render_elements::<GlesRenderer, _, _>(
                &mut frame,
                scale.x,
                &wallpaper_batch,
                &[damage],
            )?;
            if !panel_batch.is_empty() {
                draw_render_elements::<GlesRenderer, _, _>(
                    &mut frame,
                    scale.x,
                    &panel_batch,
                    &[damage],
                )?;
            }
            if !window_batch.is_empty() {
                draw_render_elements::<GlesRenderer, _, _>(
                    &mut frame,
                    scale.x,
                    &window_batch,
                    &[damage],
                )?;
            }

            let _ = frame.finish()?;

            state.send_frame_callbacks();

            if let Some(stream) = listener.accept()? {
                let data = std::sync::Arc::new(crate::state::ClientState::with_counters(
                    state.client_counters(),
                ));
                display.handle().insert_client(stream, data)?;
                state.count_client_connected();
            }
            display.dispatch_clients(&mut state)?;
            display.flush_clients()?;
        }
        backend.submit(Some(&[damage]))?;
    }
}

/// Convenience for diagnostics: the work area of a windowed run.
#[must_use]
pub fn windowed_work_area(size: Size<i32, Physical>) -> Rect {
    work_area(size.to_logical(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallpaper_bitmap_has_the_requested_dimensions() {
        let design = DesignSystem::dark();
        let wp = Wallpaper::render(&design, Size::from((64, 48)));
        assert_eq!(wp.size, Size::from((64, 48)));
    }

    #[test]
    fn wallpaper_reports_resize_correctly() {
        let wp = Wallpaper::render(&DesignSystem::dark(), Size::from((32, 32)));
        assert!(!wp.needs_resize(Size::from((32, 32))));
        assert!(wp.needs_resize(Size::from((40, 32))));
    }

    #[test]
    fn wallpaper_handles_degenerate_sizes() {
        let wp = Wallpaper::render(&DesignSystem::dark(), Size::from((0, 0)));
        assert!(!wp.needs_resize(Size::from((0, 0))));
    }

    #[test]
    fn wallpaper_pixels_match_the_design_gradient() {
        // A 1x2 bitmap samples the top and bottom of the gradient, so the
        // render path and the design system cannot silently disagree.
        let design = DesignSystem::dark();
        let wp = Wallpaper::render(&design, Size::from((1, 2)));
        assert!(!wp.needs_resize(Size::from((1, 2))));
        let top = design.wallpaper_at(0.0);
        let bottom = design.wallpaper_at(1.0);
        assert_ne!(top, bottom);
    }

    #[test]
    fn windowed_work_area_matches_headless() {
        let a = windowed_work_area(Size::from((1000, 700)));
        assert_eq!(a.y, PANEL_HEIGHT);
        assert_eq!(a.h, 700 - PANEL_HEIGHT);
    }
}
