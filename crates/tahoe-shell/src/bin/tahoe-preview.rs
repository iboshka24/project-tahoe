// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-preview`: render the Tahoe desktop chrome to a PPM image.
//!
//! Software-only, no GPU and no Wayland — see [`tahoe_shell::preview`] for
//! exactly what this does and does not prove.
//!
//! ```sh
//! cargo run -p tahoe-shell --bin tahoe-preview -- --size 1440x900 --out desktop.ppm
//! ```

use std::io::Write;

use tahoe_shell::{
    design::DesignSystem,
    preview::{PreviewWindow, render_desktop},
};

const USAGE: &str = "\
tahoe-preview — render the Tahoe desktop chrome to a PPM file

USAGE:
    tahoe-preview [OPTIONS]

OPTIONS:
    --size <WxH>            Output size (default: 1440x900)
    --appearance <light|dark>   Design appearance (default: dark)
    --out <path>            Output file, '-' for stdout (default: tahoe-preview.ppm)
    --windows <N>           Draw N sample windows in a cascade (default: 0)
    --design                Print the design tokens and exit
    -h, --help              Show this text
";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Args {
    size: (u32, u32),
    dark: bool,
    out: String,
    windows: usize,
    design: bool,
}

fn parse_args(argv: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut args = Args {
        size: (1440, 900),
        dark: true,
        out: "tahoe-preview.ppm".to_owned(),
        windows: 0,
        design: false,
    };
    let mut it = argv.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(USAGE.to_owned()),
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
            "--out" => args.out = it.next().ok_or("--out needs a value")?,
            "--windows" => {
                let v = it.next().ok_or("--windows needs a value")?;
                args.windows = v.parse().map_err(|_| format!("bad window count: {v}"))?;
            }
            "--design" => args.design = true,
            other => return Err(format!("unknown argument: {other}\n\n{USAGE}")),
        }
    }
    Ok(args)
}

/// Cascade geometry matching the compositor's placement policy.
fn sample_windows(count: usize, width: u32, height: u32, panel: i32) -> Vec<PreviewWindow> {
    let w = (width as i32 * 3 / 5).max(160);
    let h = (height as i32 * 3 / 5).max(120);
    (0..count)
        .map(|i| {
            let step = i as i32 * 38;
            let x = (96 + step).clamp(0, (width as i32 - w).max(0));
            let y = (panel + 44 + step).clamp(panel, (height as i32 - h).max(panel));
            PreviewWindow::new(x, y, w, h).focused(i + 1 == count)
        })
        .collect()
}

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
    println!("wallpaper stop samples:");
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

fn main() {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            if message.starts_with("tahoe-preview —") {
                println!("{message}");
                return;
            }
            eprintln!("{message}");
            std::process::exit(2);
        }
    };

    let design = DesignSystem::from_preference(args.dark);
    if args.design {
        print_design(&design);
        return;
    }

    let windows = sample_windows(
        args.windows,
        args.size.0,
        args.size.1,
        design.metrics.panel_height,
    );
    let image = render_desktop(&design, args.size.0, args.size.1, &windows);
    let ppm = image.to_ppm();

    if args.out == "-" {
        let mut stdout = std::io::stdout();
        if let Err(e) = stdout.write_all(&ppm) {
            eprintln!("tahoe-preview: write stdout: {e}");
            std::process::exit(1);
        }
        return;
    }

    if let Err(e) = std::fs::write(&args.out, &ppm) {
        eprintln!("tahoe-preview: write {}: {e}", args.out);
        std::process::exit(1);
    }
    println!(
        "{}: {}x{}, {} window(s), {} bytes",
        args.out,
        image.width,
        image.height,
        windows.len(),
        ppm.len()
    );
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
        assert_eq!(a.size, (1440, 900));
        assert!(a.dark);
        assert_eq!(a.windows, 0);
    }

    #[test]
    fn parses_size_and_appearance() {
        let a = args(&["--size", "800x600", "--appearance", "light"]).unwrap();
        assert_eq!(a.size, (800, 600));
        assert!(!a.dark);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(args(&["--size", "800"]).is_err());
        assert!(args(&["--size", "0x10"]).is_err());
        assert!(args(&["--appearance", "sepia"]).is_err());
        assert!(args(&["--windows", "many"]).is_err());
        assert!(args(&["--nope"]).is_err());
    }

    #[test]
    fn help_is_not_an_error_path() {
        assert!(args(&["--help"]).unwrap_err().contains("USAGE"));
    }

    #[test]
    fn sample_windows_stay_inside_the_canvas() {
        for count in [0usize, 1, 3, 10] {
            let wins = sample_windows(count, 800, 600, 28);
            assert_eq!(wins.len(), count);
            for w in wins {
                assert!(w.x >= 0 && w.y >= 28, "{w:?} escaped the work area");
                assert!(w.x + w.w <= 800, "{w:?} too wide");
                assert!(w.y + w.h <= 600, "{w:?} too tall");
            }
        }
    }

    #[test]
    fn only_the_last_sample_window_is_focused() {
        let wins = sample_windows(3, 800, 600, 28);
        assert!(!wins[0].focused);
        assert!(wins[2].focused);
    }

    #[test]
    fn sample_windows_survive_a_tiny_canvas() {
        let wins = sample_windows(2, 10, 10, 28);
        assert_eq!(wins.len(), 2);
    }
}
