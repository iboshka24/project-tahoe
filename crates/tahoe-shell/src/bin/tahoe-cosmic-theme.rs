// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-cosmic-theme`: write a Tahoe theme for the COSMIC desktop.
//!
//! COSMIC keeps its appearance in RON files addressed by config id and reads
//! system-wide defaults from `/usr/share/cosmic/`. Restyling it is therefore
//! a file-installation problem, not a fork.
//!
//! ```sh
//! # Preview the tree.
//! cargo run -p tahoe-shell --bin tahoe-cosmic-theme -- --list
//!
//! # Write it somewhere, e.g. for `install -Dm644`.
//! cargo run -p tahoe-shell --bin tahoe-cosmic-theme -- --out target/cosmic
//!
//! # Install for the current user instead (takes effect without a restart).
//! cargo run -p tahoe-shell --bin tahoe-cosmic-theme -- --apply
//! ```
//!
//! Nothing here needs root unless you are writing under `/usr/share`.

use std::{path::PathBuf, process::ExitCode};

use tahoe_shell::design::{DesignSystem, cosmic::SYSTEM_PREFIX};

const USAGE: &str = "\
tahoe-cosmic-theme — emit a Tahoe theme for the COSMIC desktop

USAGE:
    tahoe-cosmic-theme [--list] [--out <dir>] [--apply] [--appearance dark|light]

OPTIONS:
    --list                 Print the file tree and exit; write nothing
    --out <dir>            Write the tree under <dir> (for packaging)
    --apply                Write to ~/.config/cosmic, where COSMIC watches it
    --appearance <mode>    Which appearance to emit (default: dark)
    -h, --help             Show this text

WHERE IT GOES:
    --out <dir>   is for a distribution: install <dir>/* into
                  {SYSTEM_PREFIX} and every user gets it as a default.
    --apply       is for one user, and is picked up live by the config watcher.

NOTE:
    COSMIC's animation timings, its focus-indicator radius and its panel
    applet size tables are compiled into Rust and cannot be reached from a
    config file. See docs/cosmic-theming.md for the full list.
";

fn main() -> ExitCode {
    let mut list = false;
    let mut out: Option<PathBuf> = None;
    let mut apply = false;
    let mut dark = true;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--list" => list = true,
            "--apply" => apply = true,
            "--out" => match args.next() {
                Some(v) => out = Some(PathBuf::from(v)),
                None => {
                    eprintln!("tahoe-cosmic-theme: --out needs a directory\n\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "--appearance" => match args.next().as_deref() {
                Some("dark") => dark = true,
                Some("light") => dark = false,
                other => {
                    eprintln!("tahoe-cosmic-theme: unknown appearance {other:?}");
                    return ExitCode::from(2);
                }
            },
            other => {
                eprintln!("tahoe-cosmic-theme: unknown flag {other}\n\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }

    let system = DesignSystem::from_preference(dark);
    let files = system.to_cosmic_tree(&DesignSystem::from_preference(!dark));

    if list {
        println!("# {} files for {}", files.len(), system.name);
        for f in &files {
            println!("{SYSTEM_PREFIX}/{}", f.path);
        }
        return ExitCode::SUCCESS;
    }

    let root = if apply {
        match std::env::var_os("XDG_CONFIG_HOME") {
            Some(dir) => PathBuf::from(dir).join("cosmic"),
            None => match std::env::var_os("HOME") {
                Some(home) => PathBuf::from(home).join(".config/cosmic"),
                None => {
                    eprintln!("tahoe-cosmic-theme: HOME is not set; use --out instead");
                    return ExitCode::FAILURE;
                }
            },
        }
    } else if let Some(dir) = out {
        dir
    } else {
        eprintln!("tahoe-cosmic-theme: nothing to do; pass --list, --out or --apply\n\n{USAGE}");
        return ExitCode::from(2);
    };

    let mut written = 0usize;
    for f in &files {
        let path = root.join(&f.path);
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            eprintln!("tahoe-cosmic-theme: create {}: {e}", parent.display());
            return ExitCode::FAILURE;
        }
        if let Err(e) = std::fs::write(&path, &f.body) {
            eprintln!("tahoe-cosmic-theme: write {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
        written += 1;
    }
    println!("{} files written under {}", written, root.display());
    if apply {
        println!("COSMIC watches this directory, so the change applies without a restart.");
    } else {
        println!("Install with: install -Dm644 -t {SYSTEM_PREFIX}/<config-id>/v<n>/ <file>");
        println!("Or copy the tree into {SYSTEM_PREFIX}.");
    }
    ExitCode::SUCCESS
}
