// SPDX-License-Identifier: MIT OR Apache-2.0
//! The Tahoe desktop, drawn.
//!
//! This is the shell's appearance as a pure function: design system plus
//! state in, pixels out. Nothing here touches Wayland, a GPU or a clock it
//! was not handed, which is what makes it testable and what lets the same
//! code become the compositor's chrome later.
//!
//! # What a screenshot of this is, and is not
//!
//! It **is** an exact render of the shell's layout, palette, radii and type,
//! produced by the code that defines them.
//!
//! It **is not** a photograph of a running compositor. The windows drawn
//! here are placeholders, not `wl_surface`s of real clients. Saying
//! otherwise would be the kind of claim this project exists to avoid. The
//! distinction matters because the interesting failures — a client that
//! never commits a buffer, a seat that delivers no input — are exactly the
//! ones a render cannot show.

use crate::design::{DesignSystem, Rgba};
use crate::render::{Align, Canvas, Face, Fonts, Rect};

/// Everything the shell knows that is not layout: who, where, when.
///
/// Passed in rather than read, so a render is reproducible and a test does
/// not depend on the clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decor {
    /// Hostname shown in the menu bar.
    pub hostname: String,
    /// The menu bar's clock, already formatted.
    pub clock: String,
    /// Workspace names.
    pub workspaces: Vec<String>,
    /// Index of the active workspace.
    pub active_workspace: usize,
}

impl Default for Decor {
    fn default() -> Self {
        Self {
            hostname: std::fs::read_to_string("/proc/sys/kernel/hostname")
                .map(|s| s.trim().to_owned())
                .unwrap_or_else(|_| "tahoe".to_owned()),
            // A fixed time keeps renders reproducible. A real session reads
            // the clock; the renderer does not.
            clock: "Thu 15 Oct 09:41".to_owned(),
            workspaces: (1..=4).map(|i| i.to_string()).collect(),
            active_workspace: 0,
        }
    }
}

/// A window drawn by the shell.
///
/// A placeholder: a real client's pixels come from its `wl_surface`, which
/// this renderer deliberately cannot reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// Title shown in the title bar.
    pub title: String,
    /// Subtitle, normally the application.
    pub app: String,
    /// Body lines, drawn as placeholder bars.
    pub lines: Vec<String>,
}

/// A dock entry.
#[derive(Debug, Clone, PartialEq)]
pub struct DockApp {
    /// Display name.
    pub name: String,
    /// Colour of its tile, from the palette.
    pub tint: Rgba,
    /// Whether it is running, which draws the indicator dot.
    pub running: bool,
}

/// A launcher row.
#[derive(Debug, Clone, PartialEq)]
pub struct LauncherRow {
    /// Display name.
    pub name: String,
    /// Identifier, shown underneath.
    pub id: String,
    /// Tile colour.
    pub tint: Rgba,
}

/// A control-centre tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlTile {
    /// Title.
    pub title: String,
    /// State text.
    pub detail: String,
    /// What it would actually be bound to.
    pub binding: String,
    /// Whether the tile reads as on.
    pub on: bool,
}

/// What to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    /// Windows, back to front.
    pub windows: Vec<Window>,
    /// Index of the focused window.
    pub focused: Option<usize>,
    /// Dock entries.
    pub dock: Vec<DockApp>,
    /// Launcher rows, shown when [`Scene::launcher_open`].
    pub launcher: Vec<LauncherRow>,
    /// Whether the launcher is open.
    pub launcher_open: bool,
    /// Whether the control centre is open.
    pub control_center_open: bool,
    /// Control-centre tiles.
    pub control_tiles: Vec<ControlTile>,
    /// Search text shown in the launcher field.
    pub launcher_query: String,
}

impl Scene {
    /// A plausible desktop: three windows, a full dock, nothing open.
    #[must_use]
    pub fn desktop(design: &DesignSystem) -> Self {
        let a = design.accent;
        Self {
            windows: vec![
                Window {
                    title: "Universal Execution".to_owned(),
                    app: "Tahoe Files".to_owned(),
                    lines: vec![
                        "hello.exe   PE32+ x86-64   → Wine".to_owned(),
                        "game.exe    PE32+ x86-64   → Proton".to_owned(),
                        "Tool.app    Mach-O x86-64  → Darling".to_owned(),
                    ],
                },
                Window {
                    title: "compatibility-matrix.md".to_owned(),
                    app: "Tahoe Editor".to_owned(),
                    lines: vec![
                        "511 Rust tests · 21 Swift tests".to_owned(),
                        "12 formats identified".to_owned(),
                    ],
                },
                Window {
                    title: "Terminal".to_owned(),
                    app: "foot".to_owned(),
                    lines: vec![
                        "$ tahoe-uel run installer.exe".to_owned(),
                        "confidence: layer:wine".to_owned(),
                        "exit_code: 0".to_owned(),
                    ],
                },
            ],
            focused: Some(0),
            dock: vec![
                DockApp {
                    name: "Launcher".to_owned(),
                    tint: a,
                    running: false,
                },
                DockApp {
                    name: "Files".to_owned(),
                    tint: a.mix(Rgba::new(1.0, 0.8, 0.2, 1.0), 0.55),
                    running: true,
                },
                DockApp {
                    name: "Editor".to_owned(),
                    tint: a.mix(Rgba::new(0.4, 0.9, 0.5, 1.0), 0.5),
                    running: true,
                },
                DockApp {
                    name: "Terminal".to_owned(),
                    tint: design.text_muted,
                    running: true,
                },
                DockApp {
                    name: "Settings".to_owned(),
                    tint: a.mix(Rgba::new(0.8, 0.8, 0.85, 1.0), 0.6),
                    running: false,
                },
                DockApp {
                    name: "Windows".to_owned(),
                    tint: a.mix(Rgba::new(0.3, 0.6, 1.0, 1.0), 0.4),
                    running: false,
                },
            ],
            launcher: vec![
                LauncherRow {
                    name: "Tahoe Files".to_owned(),
                    id: "org.tahoe.Files".to_owned(),
                    tint: a.mix(Rgba::new(1.0, 0.8, 0.2, 1.0), 0.55),
                },
                LauncherRow {
                    name: "Tahoe Editor".to_owned(),
                    id: "org.tahoe.Editor".to_owned(),
                    tint: a.mix(Rgba::new(0.4, 0.9, 0.5, 1.0), 0.5),
                },
                LauncherRow {
                    name: "System Settings".to_owned(),
                    id: "org.tahoe.Settings".to_owned(),
                    tint: a.mix(Rgba::new(0.8, 0.8, 0.85, 1.0), 0.6),
                },
                LauncherRow {
                    name: "Universal Execution".to_owned(),
                    id: "org.tahoe.UEL".to_owned(),
                    tint: a,
                },
                LauncherRow {
                    name: "Proton".to_owned(),
                    id: "org.tahoe.Proton".to_owned(),
                    tint: a.mix(Rgba::new(0.3, 0.6, 1.0, 1.0), 0.4),
                },
            ],
            launcher_open: false,
            control_center_open: false,
            control_tiles: vec![
                ControlTile {
                    title: "Wi-Fi".to_owned(),
                    detail: "Connected".to_owned(),
                    binding: "NetworkManager".to_owned(),
                    on: true,
                },
                ControlTile {
                    title: "Bluetooth".to_owned(),
                    detail: "Off".to_owned(),
                    binding: "BlueZ".to_owned(),
                    on: false,
                },
                ControlTile {
                    title: "Appearance".to_owned(),
                    detail: "Dark".to_owned(),
                    binding: "portal Settings".to_owned(),
                    on: true,
                },
                ControlTile {
                    title: "Sound".to_owned(),
                    detail: "72%".to_owned(),
                    binding: "PipeWire".to_owned(),
                    on: true,
                },
            ],
            launcher_query: String::new(),
        }
    }

    /// The same desktop with the launcher open and a query typed.
    #[must_use]
    pub fn with_launcher(mut self) -> Self {
        self.launcher_open = true;
        self.launcher_query = "tahoe".to_owned();
        self
    }

    /// The same desktop with the control centre open.
    #[must_use]
    pub fn with_control_center(mut self) -> Self {
        self.control_center_open = true;
        self
    }
}

/// Metrics the renderer derives from the design system.
struct Layout {
    panel: Rect,
    work: Rect,
    dock: Rect,
    panel_height: i32,
    spacing: i32,
    radius: i32,
}

fn layout(design: &DesignSystem, width: i32, height: i32) -> Layout {
    let panel_height = design.metrics.panel_height;
    let spacing = design.spacing.md;
    let dock_h = design.metrics.dock_height;
    let dock_w = (design.metrics.dock_icon * 6 + spacing * 7).min(width - design.spacing.xl * 2);
    Layout {
        panel: Rect::new(0, 0, width, panel_height),
        work: Rect::new(
            0,
            panel_height,
            width,
            height - panel_height - dock_h - spacing * 2,
        ),
        dock: Rect::new(
            (width - dock_w) / 2,
            height - dock_h - design.spacing.lg,
            dock_w,
            dock_h,
        ),
        panel_height,
        spacing,
        radius: design.radii.window,
    }
}

/// Draw the desktop.
#[must_use]
pub fn render(design: &DesignSystem, decor: &Decor, scene: &Scene, fonts: &Fonts) -> Canvas {
    let width = 1600u32;
    let height = 1000u32;
    render_sized(design, decor, scene, fonts, width, height)
}

/// Draw the desktop at an explicit size.
#[must_use]
pub fn render_sized(
    design: &DesignSystem,
    decor: &Decor,
    scene: &Scene,
    fonts: &Fonts,
    width: u32,
    height: u32,
) -> Canvas {
    let mut canvas = Canvas::new(width, height);
    // Clone the arc handles; a Canvas owns its fonts so text can be drawn
    // without threading them through every call.
    let fonts = Fonts {
        regular: fonts.regular.clone(),
        bold: fonts.bold.clone(),
        sources: fonts.sources.clone(),
    };
    let mut canvas = {
        let taken = std::mem::replace(&mut canvas, Canvas::new(0, 0));
        taken.with_fonts(fonts)
    };
    let l = layout(design, width as i32, height as i32);

    draw_wallpaper(&mut canvas, design, width, height);
    draw_windows(&mut canvas, design, scene, &l);
    draw_panel(&mut canvas, design, decor, &l);
    if scene.launcher_open {
        draw_launcher(&mut canvas, design, scene, &l);
    }
    if scene.control_center_open {
        draw_control_center(&mut canvas, design, scene, &l);
    }
    draw_dock(&mut canvas, design, scene, &l);
    canvas
}

fn draw_wallpaper(canvas: &mut Canvas, design: &DesignSystem, width: u32, height: u32) {
    canvas.fill_gradient(Rect::new(0, 0, width as i32, height as i32), |t| Rgba {
        a: 1.0,
        ..design.wallpaper_at(t)
    });
}

fn draw_panel(canvas: &mut Canvas, design: &DesignSystem, decor: &Decor, l: &Layout) {
    let text = design.text;
    // The panel is a translucent material over the wallpaper, so the
    // wallpaper shows through the way it will in a real session.
    canvas.fill_rect(l.panel, design.panel);
    canvas.fill_rect(
        Rect::new(0, l.panel_height - 1, l.panel.w, 1),
        design.separator,
    );

    let h = l.panel_height;
    let mut x = l.spacing;
    // A mark rather than a logo: an approximation of the platform glyph
    // would be both trademark trouble and worse than nothing.
    canvas.fill_rounded(Rect::new(x, h / 2 - 5, 10, 10), 5, design.accent);
    x += 18;
    canvas.draw_text(
        "Tahoe",
        Rect::new(x, 0, 60, h),
        13.0,
        Face::Bold,
        text,
        Align::Left,
    );
    let (name_w, _) = canvas.measure("Tahoe", 13.0, Face::Bold);
    x += name_w + l.spacing;
    for item in ["File", "Edit", "View", "Window", "Help"] {
        canvas.draw_text(
            item,
            Rect::new(x, 0, 60, h),
            12.5,
            Face::Regular,
            design.text_muted,
            Align::Left,
        );
        let (w, _) = canvas.measure(item, 12.5, Face::Regular);
        x += w + l.spacing + 2;
    }

    // Right side, laid out from the edge inwards.
    let mut right = l.panel.w - l.spacing;
    let (clock_w, _) = canvas.measure(&decor.clock, 12.0, Face::Regular);
    right -= clock_w;
    canvas.draw_text(
        &decor.clock,
        Rect::new(right, 0, clock_w, h),
        12.0,
        Face::Regular,
        text,
        Align::Left,
    );
    right -= l.spacing;

    let (host_w, _) = canvas.measure(&decor.hostname, 12.0, Face::Regular);
    right -= host_w;
    canvas.draw_text(
        &decor.hostname,
        Rect::new(right, 0, host_w, h),
        12.0,
        Face::Regular,
        design.text_muted,
        Align::Left,
    );
    right -= l.spacing;

    // Battery and signal, drawn as shapes: no icon font is assumed.
    canvas.stroke_rounded(
        Rect::new(right - 22, h / 2 - 5, 22, 10),
        3,
        design.text_muted,
    );
    canvas.fill_rounded(
        Rect::new(right - 20, h / 2 - 3, 14, 6),
        2,
        design.text_muted,
    );
    canvas.fill_rounded(
        Rect::new(right - 26, h / 2 - 5, 2, 10),
        1,
        design.text_muted,
    );
    right -= 34;
    for i in 0..3i32 {
        let bar_h = 3 + i * 3;
        canvas.fill_rounded(
            Rect::new(right - 16 + i * 5, h / 2 + 4 - bar_h, 3, bar_h),
            1,
            design.text_muted,
        );
    }
    right -= 30;

    // Control centre toggle.
    canvas.fill_rounded(
        Rect::new(right - 22, h / 2 - 5, 22, 10),
        5,
        design.text_muted.with_alpha(0.45),
    );
    right -= 30;

    // Workspace pills.
    for (i, name) in decor.workspaces.iter().enumerate() {
        let (w, _) = canvas.measure(name, 11.0, Face::Bold);
        let pill = Rect::new(right - w - 10, h / 2 - 8, w + 10, 16);
        if i == decor.active_workspace {
            canvas.fill_rounded(pill, 4, design.accent);
        }
        let colour = if i == decor.active_workspace {
            Rgba::new(1.0, 1.0, 1.0, 1.0)
        } else {
            design.text_muted
        };
        canvas.draw_text(name, pill, 11.0, Face::Bold, colour, Align::Center);
        right -= pill.w + 4;
    }
}

fn draw_windows(canvas: &mut Canvas, design: &DesignSystem, scene: &Scene, l: &Layout) {
    let base_w = (l.work.w as f32 * 0.46) as i32;
    let base_h = (l.work.h as f32 * 0.58) as i32;
    let step = 46;
    for (i, window) in scene.windows.iter().enumerate() {
        let focused = scene.focused == Some(i);
        let x = l.work.x + l.spacing + (i as i32) * step;
        let y = l.work.y + l.spacing + (i as i32) * step;
        let rect = Rect::new(x, y, base_w, base_h);

        canvas.drop_shadow(
            rect,
            l.radius,
            if focused { 14 } else { 8 },
            if focused { 1.0 } else { 0.7 },
        );
        canvas.fill_rounded(rect, l.radius, design.window_background);

        // Title bar.
        let bar = Rect::new(rect.x, rect.y, rect.w, 32);
        canvas.fill_rounded(bar, l.radius, design.window_background);
        canvas.fill_rect(
            Rect::new(bar.x, bar.y + l.radius, bar.w, bar.h - l.radius),
            design.window_background,
        );

        // Traffic lights: the three colours a macOS window shows. Drawn even
        // though Tahoe has no server-side decoration yet, because the
        // reference design has them.
        for (j, colour) in [
            Rgba::new(1.0, 0.37, 0.34, 1.0),
            Rgba::new(1.0, 0.74, 0.20, 1.0),
            Rgba::new(0.24, 0.79, 0.30, 1.0),
        ]
        .iter()
        .enumerate()
        {
            canvas.fill_rounded(
                Rect::new(bar.x + 14 + (j as i32) * 18, bar.y + 11, 11, 11),
                6,
                *colour,
            );
        }
        canvas.draw_text(
            &window.title,
            bar,
            12.5,
            if focused { Face::Bold } else { Face::Regular },
            if focused {
                design.text
            } else {
                design.text_muted
            },
            Align::Center,
        );

        canvas.fill_rect(
            Rect::new(rect.x, bar.y + bar.h, rect.w, 1),
            design.separator,
        );

        // Body: the app name, then the lines. A real window shows a client's
        // pixels here.
        canvas.draw_text(
            &window.app,
            Rect::new(
                rect.x + l.spacing,
                bar.y + bar.h + l.spacing,
                rect.w - l.spacing * 2,
                18,
            ),
            12.0,
            Face::Bold,
            design.accent,
            Align::Left,
        );
        for (j, line) in window.lines.iter().enumerate() {
            let line_rect = Rect::new(
                rect.x + l.spacing,
                bar.y + bar.h + l.spacing + 26 + (j as i32) * 22,
                rect.w - l.spacing * 2,
                18,
            );
            canvas.draw_text(
                line,
                line_rect,
                11.5,
                Face::Regular,
                design.text_muted,
                Align::Left,
            );
        }

        canvas.stroke_rounded(
            rect,
            l.radius,
            if focused {
                design.accent
            } else {
                design.separator
            },
        );
    }
}

fn draw_dock(canvas: &mut Canvas, design: &DesignSystem, scene: &Scene, l: &Layout) {
    let dock = l.dock;
    canvas.drop_shadow(dock, design.radii.dock, 16, 1.0);
    canvas.fill_rounded(dock, design.radii.dock, design.dock);
    canvas.stroke_rounded(dock, design.radii.dock, design.separator);

    let icon = design.metrics.dock_icon;
    let count = scene.dock.len() as i32;
    if count == 0 {
        return;
    }
    let total = count * icon + (count - 1) * l.spacing;
    let mut x = dock.x + (dock.w - total) / 2;
    let y = dock.y + (dock.h - icon) / 2 - 4;
    for app in &scene.dock {
        let tile = Rect::new(x, y, icon, icon);
        canvas.fill_rounded(tile, design.radii.card, app.tint.with_alpha(0.22));
        canvas.stroke_rounded(tile, design.radii.card, app.tint.with_alpha(0.55));
        // An initial rather than an icon: there is no icon theme here, and a
        // drawn approximation would be a worse lie than a letter.
        let initial = app
            .name
            .chars()
            .next()
            .map(|c| c.to_string())
            .unwrap_or_default();
        canvas.fill_rounded(
            Rect::new(tile.x + 8, tile.y + 8, icon - 16, icon - 16),
            6,
            app.tint.with_alpha(0.75),
        );
        canvas.draw_text(
            &initial,
            tile,
            (icon as f32) * 0.34,
            Face::Bold,
            Rgba::new(1.0, 1.0, 1.0, 0.95),
            Align::Center,
        );
        if app.running {
            canvas.fill_rounded(
                Rect::new(tile.x + icon / 2 - 2, dock.y + dock.h - 9, 4, 4),
                2,
                design.accent,
            );
        }
        x += icon + l.spacing;
    }
}

fn draw_launcher(canvas: &mut Canvas, design: &DesignSystem, scene: &Scene, l: &Layout) {
    let width = 460;
    let rows = scene.launcher.len().max(1) as i32;
    let height = 96 + rows * 44;
    let rect = Rect::new(
        (l.panel.w - width) / 2,
        l.panel_height + l.spacing * 2,
        width,
        height,
    );

    // The scrim: everything behind is dimmed, which is what makes a launcher
    // read as modal without stealing focus in a way this renderer could not
    // represent anyway.
    for y in l.panel_height..l.panel_height + l.work.h {
        for x in 0..l.panel.w {
            canvas.blend(x, y, Rgba::new(0.0, 0.0, 0.0, 0.45), 1.0);
        }
    }

    canvas.drop_shadow(rect, design.radii.window, 26, 1.4);
    canvas.fill_rounded(rect, design.radii.window, design.window_background);
    canvas.stroke_rounded(rect, design.radii.window, design.accent.with_alpha(0.35));

    // Search field.
    let field = Rect::new(
        rect.x + l.spacing,
        rect.y + l.spacing,
        rect.w - l.spacing * 2,
        34,
    );
    canvas.fill_rounded(
        field,
        design.radii.control,
        design.text_muted.with_alpha(0.14),
    );
    canvas.fill_rounded(
        Rect::new(field.x + 12, field.y + 13, 8, 8),
        4,
        design.text_muted,
    );
    canvas.fill_rect(
        Rect::new(field.x + 19, field.y + 20, 5, 1),
        design.text_muted,
    );
    let query = if scene.launcher_query.is_empty() {
        "Search".to_owned()
    } else {
        scene.launcher_query.clone()
    };
    let colour = if scene.launcher_query.is_empty() {
        design.text_muted.with_alpha(0.7)
    } else {
        design.text
    };
    canvas.draw_text(
        &query,
        Rect::new(field.x + 32, field.y, field.w - 44, field.h),
        13.5,
        Face::Regular,
        colour,
        Align::Left,
    );

    // Rows.
    for (i, row) in scene.launcher.iter().enumerate() {
        let y = rect.y + l.spacing + 44 + (i as i32) * 44;
        let hit = !scene.launcher_query.is_empty()
            && (row
                .name
                .to_lowercase()
                .contains(&scene.launcher_query.to_lowercase())
                || row
                    .id
                    .to_lowercase()
                    .contains(&scene.launcher_query.to_lowercase()));
        let row_rect = Rect::new(rect.x + l.spacing - 4, y, rect.w - l.spacing * 2 + 8, 40);
        canvas.fill_rounded(
            row_rect,
            design.radii.card,
            design.accent.with_alpha(if hit { 0.20 } else { 0.06 }),
        );

        let tile = Rect::new(row_rect.x + 8, y + 6, 28, 28);
        canvas.fill_rounded(tile, 7, row.tint.with_alpha(0.85));
        let initial = row
            .name
            .chars()
            .next()
            .map(|c| c.to_string())
            .unwrap_or_default();
        canvas.draw_text(
            &initial,
            tile,
            14.0,
            Face::Bold,
            Rgba::new(1.0, 1.0, 1.0, 0.95),
            Align::Center,
        );

        canvas.draw_text(
            &row.name,
            Rect::new(tile.x + 40, y + 4, 260, 18),
            13.0,
            Face::Regular,
            design.text,
            Align::Left,
        );
        canvas.draw_text(
            &row.id,
            Rect::new(tile.x + 40, y + 21, 260, 14),
            10.5,
            Face::Regular,
            design.text_muted,
            Align::Left,
        );
        if hit {
            canvas.fill_rounded(
                Rect::new(row_rect.x + row_rect.w - 22, y + 18, 6, 6),
                3,
                design.accent,
            );
        }
    }
}

fn draw_control_center(canvas: &mut Canvas, design: &DesignSystem, scene: &Scene, l: &Layout) {
    let cols = 2;
    let tile_w = 190;
    let tile_h = 84;
    let rows = (scene.control_tiles.len() as i32 + cols - 1) / cols;
    let width = cols * tile_w + l.spacing * (cols + 1);
    let height = rows * tile_h + l.spacing * (rows + 1);
    let rect = Rect::new(
        l.panel.w - width - l.spacing * 2,
        l.panel_height + l.spacing,
        width,
        height,
    );

    canvas.drop_shadow(rect, design.radii.window, 22, 1.2);
    canvas.fill_rounded(
        rect,
        design.radii.window,
        design.window_background.with_alpha(0.97),
    );
    canvas.stroke_rounded(rect, design.radii.window, design.separator);

    for (i, tile) in scene.control_tiles.iter().enumerate() {
        let col = (i as i32) % cols;
        let row = (i as i32) / cols;
        let tile_rect = Rect::new(
            rect.x + l.spacing + col * (tile_w + l.spacing),
            rect.y + l.spacing + row * (tile_h + l.spacing),
            tile_w,
            tile_h,
        );
        let tint = if tile.on {
            design.accent
        } else {
            design.text_muted
        };
        canvas.fill_rounded(
            tile_rect,
            design.radii.card,
            tint.with_alpha(if tile.on { 0.18 } else { 0.10 }),
        );
        canvas.stroke_rounded(tile_rect, design.radii.card, tint.with_alpha(0.25));
        canvas.fill_rounded(
            Rect::new(tile_rect.x + 12, tile_rect.y + 12, 14, 14),
            7,
            tint.with_alpha(0.9),
        );
        canvas.draw_text(
            &tile.title,
            Rect::new(tile_rect.x + 34, tile_rect.y + 8, 130, 22),
            12.5,
            Face::Bold,
            design.text,
            Align::Left,
        );
        canvas.draw_text(
            &tile.detail,
            Rect::new(tile_rect.x + 34, tile_rect.y + 28, 130, 16),
            11.0,
            Face::Regular,
            design.text_muted,
            Align::Left,
        );
        // The binding is named, so nobody mistakes a drawn tile for a
        // working switch.
        canvas.draw_text(
            &tile.binding,
            Rect::new(tile_rect.x + 12, tile_rect.y + tile_h - 22, tile_w - 24, 14),
            9.5,
            Face::Regular,
            design.text_muted.with_alpha(0.75),
            Align::Left,
        );
    }
}

#[cfg(test)]
mod tests;
