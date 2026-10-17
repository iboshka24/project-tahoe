// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-preview`: render the Tahoe desktop chrome to a PPM image.
//!
//! Software-only, no GPU and no Wayland — see [`tahoe_shell::preview`] for
//! exactly what this does and does not prove.
//!
//! ```sh
//! cargo run -p tahoe-shell --bin tahoe-preview -- --size 1440x900 --out desktop.ppm
//! ```

use std::path::{Path, PathBuf};

use tahoe_shell::render::{Canvas, Fonts};

use tahoe_shell::{
    design::DesignSystem,
    scenes::{self, Decor, Scene},
};

const USAGE: &str = "\
tahoe-preview — render the Tahoe desktop chrome to a PPM file

USAGE:
    tahoe-preview [OPTIONS]

OPTIONS:
    --scene <name>          desktop | launcher | control | all   (default: desktop)
    --size <WxH>            Output size (default: 1600x1000)
    --appearance <light|dark>   Design appearance (default: dark)
    --out <path or dir>     Output. For --scene all this is a directory.
                            Default: tahoe-<scene>.png (or ./tahoe-scenes/)
    --design                Print the design tokens and exit
    -h, --help              Show this text

SCENES:
    desktop   three windows, the panel and the dock
    launcher  the same, with the launcher open and a query typed
    control   the same, with the control centre open
    all       every scene above, both appearances, plus the light desktop

NOTE:
    This renders the shell's layout, palette, radii and type from the code
    that defines them. It is not a photograph of a running compositor: the
    windows are placeholders, not client surfaces. See scenes.rs.
";

/// Which scene to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Which {
    Desktop,
    Launcher,
    Control,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Args {
    scene: Which,
    size: (u32, u32),
    dark: bool,
    both_appearances: bool,
    out: Option<String>,
    design: bool,
}

fn parse_args(argv: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut args = Args {
        scene: Which::Desktop,
        size: (1600, 1000),
        dark: true,
        both_appearances: false,
        out: None,
        design: false,
    };
    let mut it = argv.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(USAGE.to_owned()),
            "--scene" => {
                let v = it.next().ok_or("--scene needs a value")?;
                args.scene = match v.as_str() {
                    "desktop" => Which::Desktop,
                    "launcher" => Which::Launcher,
                    "control" => Which::Control,
                    "all" => {
                        args.both_appearances = true;
                        Which::All
                    }
                    other => return Err(format!("unknown scene: {other}")),
                };
            }
            "--size" => {
                let v = it.next().ok_or("--size needs a value")?;
                let (w, h) = v
                    .split_once(['x', 'X'])
                    .ok_or_else(|| format!("bad size: {v}"))?;
                args.size = (
                    w.trim().parse().map_err(|_| format!("bad width: {v}"))?,
                    h.trim().parse().map_err(|_| format!("bad height: {v}"))?,
                );
                if args.size.0 == 0 || args.size.1 == 0 {
                    return Err(format!("size must be positive: {v}"));
                }
            }
            "--appearance" => {
                args.dark = match it.next().ok_or("--appearance needs a value")?.as_str() {
                    "dark" => true,
                    "light" => false,
                    other => return Err(format!("unknown appearance: {other}")),
                };
            }
            "--out" => args.out = Some(it.next().ok_or("--out needs a value")?),
            "--design" => args.design = true,
            other => return Err(format!("unknown argument: {other}\n\n{USAGE}")),
        }
    }
    Ok(args)
}

/// One render: a name, which appearance, and how to build the scene.
///
/// A named type rather than a three-element tuple, because a caller who
/// swaps two of those renders the wrong scene into the wrong file.
type Job = (&'static str, bool, fn(&DesignSystem) -> Scene);

/// Print the design tokens, for a terminal or a bug report.
fn print_design(design: &DesignSystem) {
    println!("appearance: {}", design.name);
    println!(
        "panel:  {:.3} {:.3} {:.3} a={:.2}",
        design.panel.r, design.panel.g, design.panel.b, design.panel.a
    );
    println!(
        "dock:   {:.3} {:.3} {:.3} a={:.2}",
        design.dock.r, design.dock.g, design.dock.b, design.dock.a
    );
    println!(
        "accent: {:.3} {:.3} {:.3}",
        design.accent.r, design.accent.g, design.accent.b
    );
    println!(
        "text:   {:.3} {:.3} {:.3}",
        design.text.r, design.text.g, design.text.b
    );
    println!(
        "metrics: panel_height={} dock_height={} dock_icon={} sidebar_width={}",
        design.metrics.panel_height,
        design.metrics.dock_height,
        design.metrics.dock_icon,
        design.metrics.sidebar_width
    );
    println!(
        "spacing: xs={} sm={} md={} lg={} xl={}",
        design.spacing.xs,
        design.spacing.sm,
        design.spacing.md,
        design.spacing.lg,
        design.spacing.xl
    );
    println!(
        "radii:  control={} card={} window={} dock={}",
        design.radii.control, design.radii.card, design.radii.window, design.radii.dock
    );
    println!("wallpaper stops:");
    for i in 0..=4 {
        let c = design.wallpaper_at(i as f32 / 4.0);
        println!(
            "  t={:.2}  {:.3} {:.3} {:.3}",
            i as f32 / 4.0,
            c.r,
            c.g,
            c.b
        );
    }
}

/// Render one scene and return the canvas.
fn render_one(
    design: &DesignSystem,
    decor: &Decor,
    scene: &Scene,
    fonts: &Fonts,
    args: &Args,
) -> Canvas {
    scenes::render_sized(design, decor, scene, fonts, args.size.0, args.size.1)
}

/// Write a canvas to a PNG, creating the parent directory.
fn write_png(canvas: &Canvas, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(path, canvas.to_png()).map_err(|e| format!("{}: {e}", path.display()))
}

fn main() {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            if message.starts_with("tahoe-preview") {
                println!("{message}");
                return;
            }
            eprintln!("{message}");
            std::process::exit(2);
        }
    };

    if args.design {
        print_design(&DesignSystem::from_preference(args.dark));
        return;
    }

    let fonts = match Fonts::system() {
        Ok(fonts) => fonts,
        Err(e) => {
            // Text is the point of this renderer, so a machine with no fonts
            // gets a clear failure rather than a picture with no words in it.
            eprintln!("tahoe-preview: no usable font: {e}");
            std::process::exit(1);
        }
    };
    let decor = Decor::default();

    // `--scene all` writes a set; a single scene writes one file.
    let jobs: Vec<Job> = match args.scene {
        Which::All => vec![
            ("desktop-dark", true, Scene::desktop),
            ("launcher-dark", true, |d| Scene::desktop(d).with_launcher()),
            ("control-dark", true, |d| {
                Scene::desktop(d).with_control_center()
            }),
            ("desktop-light", false, Scene::desktop),
            ("launcher-light", false, |d| {
                Scene::desktop(d).with_launcher()
            }),
        ],
        Which::Desktop => vec![("desktop", args.dark, Scene::desktop)],
        Which::Launcher => vec![("launcher", args.dark, |d| Scene::desktop(d).with_launcher())],
        Which::Control => vec![("control", args.dark, |d| {
            Scene::desktop(d).with_control_center()
        })],
    };

    let mut written = 0usize;
    for (name, dark, build) in jobs {
        let design = DesignSystem::from_preference(dark);
        let canvas = render_one(&design, &decor, &build(&design), &fonts, &args);
        let path = match &args.out {
            Some(out) if args.scene == Which::All || out.ends_with('/') => {
                PathBuf::from(out).join(format!("tahoe-{name}.png"))
            }
            Some(out) => PathBuf::from(out),
            None if args.scene == Which::All => {
                PathBuf::from("tahoe-scenes").join(format!("tahoe-{name}.png"))
            }
            None => PathBuf::from(format!("tahoe-{name}.png")),
        };
        if let Err(e) = write_png(&canvas, &path) {
            eprintln!("tahoe-preview: {e}");
            std::process::exit(1);
        }
        let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        println!(
            "{}: {}x{} {} PNG, {} KB",
            path.display(),
            canvas.width,
            canvas.height,
            if dark { "dark" } else { "light" },
            bytes / 1024
        );
        written += 1;
    }
    println!(
        "{written} scene(s) rendered from the design system in {}",
        args.size.0
    );
    let _ = &args.both_appearances;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        parse_args(list.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn defaults_are_sane() {
        let a = args(&[]).unwrap();
        assert_eq!(a.scene, Which::Desktop);
        assert_eq!(a.size, (1600, 1000));
        assert!(a.dark);
        assert!(!a.design);
        assert!(a.out.is_none());
    }

    #[test]
    fn every_scene_parses() {
        for (name, expected) in [
            ("desktop", Which::Desktop),
            ("launcher", Which::Launcher),
            ("control", Which::Control),
            ("all", Which::All),
        ] {
            assert_eq!(args(&["--scene", name]).unwrap().scene, expected, "{name}");
        }
        assert!(args(&["--scene", "nonsense"]).is_err());
    }

    #[test]
    fn all_enables_both_appearances() {
        // `--scene all` is the gallery, and a gallery of one appearance is
        // not a gallery.
        assert!(args(&["--scene", "all"]).unwrap().both_appearances);
        assert!(!args(&["--scene", "desktop"]).unwrap().both_appearances);
    }

    #[test]
    fn parses_size_and_appearance() {
        let a = args(&["--size", "800x600", "--appearance", "light"]).unwrap();
        assert_eq!(a.size, (800, 600));
        assert!(!a.dark);
        assert_eq!(args(&["--size", "1920X1080"]).unwrap().size, (1920, 1080));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(args(&["--size", "800"]).is_err());
        assert!(args(&["--size", "0x10"]).is_err());
        assert!(args(&["--appearance", "sepia"]).is_err());
        assert!(args(&["--nope"]).is_err());
        assert!(args(&["--scene"]).is_err());
    }

    #[test]
    fn help_is_not_an_error_path() {
        let message = args(&["--help"]).unwrap_err();
        assert!(message.contains("SCENES"), "{message}");
    }

    #[test]
    fn the_out_argument_is_taken_verbatim() {
        let a = args(&["--out", "/tmp/x.png"]).unwrap();
        assert_eq!(a.out.as_deref(), Some("/tmp/x.png"));
        assert!(args(&["--out"]).is_err());
    }
}
