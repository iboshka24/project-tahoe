// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-compositor` entry point.
//!
//! Two backends, one protocol surface:
//!
//! ```text
//! tahoe-compositor --backend headless --socket /tmp/tahoe.sock --describe
//! tahoe-compositor --backend winit
//! ```
//!
//! `headless` serves real Wayland clients with no display attached; it is
//! what the test suite and CI use. `winit` opens a nested window and
//! renders the real desktop.

use std::time::Duration;

use tahoe_compositor::backend::{SocketSpec, headless::HeadlessCompositor};
use tracing_subscriber::EnvFilter;

/// Parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Args {
    backend: Backend,
    socket: Option<String>,
    size: (i32, i32),
    exit_after: Option<Duration>,
    describe: bool,
    appearance: Option<Appearance>,
    no_panel: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    Winit,
    Headless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Appearance {
    Light,
    Dark,
}

const USAGE: &str = "\
tahoe-compositor — Tahoe Wayland compositor

USAGE:
    tahoe-compositor [OPTIONS]

OPTIONS:
    --backend <winit|headless>   Where frames go (default: winit)
    --socket <name|path>         Wayland socket (default: tahoe-0)
    --size <WxH>                 Output size (default: 1440x900)
    --exit-after <seconds>       Quit automatically (smoke tests)
    --describe                   Print state, then exit
    --appearance <light|dark>    Override the design system appearance
    --no-panel                   Do not draw the reserved panel strip
    -h, --help                   Show this text

EXAMPLES:
    tahoe-compositor --backend headless --socket /tmp/tahoe.sock --exit-after 5
    WAYLAND_DISPLAY=tahoe-0 foot
";

fn parse_size(value: &str) -> Result<(i32, i32), String> {
    let (w, h) = value
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("bad size: {value}"))?;
    let w: i32 = w
        .trim()
        .parse()
        .map_err(|_| format!("bad width in {value}"))?;
    let h: i32 = h
        .trim()
        .parse()
        .map_err(|_| format!("bad height in {value}"))?;
    if w <= 0 || h <= 0 {
        return Err(format!("size must be positive: {value}"));
    }
    Ok((w, h))
}

fn parse_args(argv: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut args = Args {
        backend: Backend::Winit,
        socket: None,
        size: (1440, 900),
        exit_after: None,
        describe: false,
        appearance: None,
        no_panel: false,
    };
    let mut it = argv.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(USAGE.to_owned()),
            "--backend" => {
                let v = it.next().ok_or("--backend needs a value")?;
                args.backend = match v.as_str() {
                    "winit" => Backend::Winit,
                    "headless" => Backend::Headless,
                    other => return Err(format!("unknown backend: {other}")),
                };
            }
            "--socket" => args.socket = Some(it.next().ok_or("--socket needs a value")?),
            "--size" => {
                args.size = parse_size(&it.next().ok_or("--size needs a value")?)?;
            }
            "--exit-after" => {
                let v = it.next().ok_or("--exit-after needs a value")?;
                let secs: f64 = v.parse().map_err(|_| format!("bad seconds: {v}"))?;
                if !secs.is_finite() || secs < 0.0 {
                    return Err(format!("bad seconds: {v}"));
                }
                args.exit_after = Some(Duration::from_secs_f64(secs));
            }
            "--describe" => args.describe = true,
            "--no-panel" => args.no_panel = true,
            "--appearance" => {
                let v = it.next().ok_or("--appearance needs a value")?;
                args.appearance = Some(match v.as_str() {
                    "light" => Appearance::Light,
                    "dark" => Appearance::Dark,
                    other => return Err(format!("unknown appearance: {other}")),
                });
            }
            other => return Err(format!("unknown argument: {other}\n\n{USAGE}")),
        }
    }
    Ok(args)
}

fn socket_spec(args: &Args) -> SocketSpec {
    match &args.socket {
        Some(s) if s.starts_with('/') => SocketSpec::Path(s.into()),
        Some(name) => SocketSpec::Named(name.clone()),
        None => SocketSpec::Named("tahoe-0".to_owned()),
    }
}

fn main() {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            // `--help` lands here too: it is not an error, but it is not a
            // runnable configuration either.
            if message.starts_with("tahoe-compositor —") {
                println!("{message}");
                return;
            }
            eprintln!("{message}");
            std::process::exit(2);
        }
    };

    match args.appearance {
        Some(Appearance::Light) => {
            // SAFETY: single-threaded, before any worker starts.
            unsafe { std::env::set_var("TAHOE_APPEARANCE", "light") };
        }
        Some(Appearance::Dark) => {
            unsafe { std::env::set_var("TAHOE_APPEARANCE", "dark") };
        }
        None => {}
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let result = match args.backend {
        Backend::Headless => run_headless(&args),
        Backend::Winit => run_winit(&args),
    };
    if let Err(error) = result {
        eprintln!("tahoe-compositor: {error}");
        std::process::exit(1);
    }
}

fn run_headless(args: &Args) -> Result<(), String> {
    let mut compositor =
        HeadlessCompositor::bind(socket_spec(args), args.size.into()).map_err(|e| e.to_string())?;
    if let Some(path) = compositor.socket_path() {
        println!("WAYLAND_DISPLAY={}", path.display());
    } else if let SocketSpec::Named(name) = socket_spec(args) {
        println!("WAYLAND_DISPLAY={name}");
    }
    if args.describe {
        print!("{}", compositor.state.describe());
        return Ok(());
    }
    let deadline = args.exit_after.map(|d| std::time::Instant::now() + d);
    compositor
        .run(|state| {
            deadline.is_some_and(|d| std::time::Instant::now() >= d)
                || state.stats.toplevels_created > 0
        })
        .map_err(|e| e.to_string())?;
    print!("{}", compositor.state.describe());
    Ok(())
}

#[cfg(feature = "winit")]
fn run_winit(args: &Args) -> Result<(), String> {
    let options = tahoe_compositor::backend::winit::Options {
        exit_after: args.exit_after,
        draw_panel: !args.no_panel,
        force_light: matches!(args.appearance, Some(Appearance::Light)),
    };
    tahoe_compositor::backend::winit::run(socket_spec(args), options).map_err(|e| e.to_string())
}

#[cfg(not(feature = "winit"))]
fn run_winit(_args: &Args) -> Result<(), String> {
    Err("this build was compiled without the `winit` feature; use --backend headless".to_owned())
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
        assert_eq!(a.backend, Backend::Winit);
        assert_eq!(a.size, (1440, 900));
        assert!(a.exit_after.is_none());
        assert!(!a.describe);
    }

    #[test]
    fn parses_headless_with_path_socket() {
        let a = args(&["--backend", "headless", "--socket", "/tmp/x.sock"]).unwrap();
        assert_eq!(a.backend, Backend::Headless);
        assert_eq!(socket_spec(&a), SocketSpec::Path("/tmp/x.sock".into()));
    }

    #[test]
    fn relative_socket_is_a_name() {
        let a = args(&["--socket", "wayland-7"]).unwrap();
        assert_eq!(socket_spec(&a), SocketSpec::Named("wayland-7".into()));
    }

    #[test]
    fn default_socket_is_tahoe_0() {
        assert_eq!(
            socket_spec(&args(&[]).unwrap()),
            SocketSpec::Named("tahoe-0".into())
        );
    }

    #[test]
    fn parses_size_in_both_separators() {
        assert_eq!(args(&["--size", "800x600"]).unwrap().size, (800, 600));
        assert_eq!(args(&["--size", "1920X1080"]).unwrap().size, (1920, 1080));
    }

    #[test]
    fn rejects_bad_sizes() {
        assert!(parse_size("800").is_err());
        assert!(parse_size("0x100").is_err());
        assert!(parse_size("-4x10").is_err());
        assert!(parse_size("axb").is_err());
    }

    #[test]
    fn parses_exit_after_fractional_seconds() {
        let a = args(&["--exit-after", "0.25"]).unwrap();
        assert_eq!(a.exit_after, Some(Duration::from_millis(250)));
    }

    #[test]
    fn rejects_negative_or_nan_seconds() {
        assert!(args(&["--exit-after", "-1"]).is_err());
        assert!(args(&["--exit-after", "nan"]).is_err());
    }

    #[test]
    fn rejects_unknown_backend_and_flags() {
        assert!(args(&["--backend", "x11"]).is_err());
        assert!(args(&["--wat"]).is_err());
        assert!(args(&["--socket"]).is_err());
    }

    #[test]
    fn help_is_returned_not_panicked() {
        let err = args(&["--help"]).unwrap_err();
        assert!(err.contains("USAGE"));
    }

    #[test]
    fn appearance_parses() {
        assert_eq!(
            args(&["--appearance", "light"]).unwrap().appearance,
            Some(Appearance::Light)
        );
        assert_eq!(
            args(&["--appearance", "dark"]).unwrap().appearance,
            Some(Appearance::Dark)
        );
        assert!(args(&["--appearance", "sepia"]).is_err());
    }

    #[test]
    fn no_panel_flag() {
        assert!(args(&["--no-panel"]).unwrap().no_panel);
    }
}
