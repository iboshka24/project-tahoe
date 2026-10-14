// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-design-export`: write the SwiftUI copy of the design system.
//!
//! ```sh
//! # Write the file the SwiftUI app imports.
//! cargo run -p tahoe-shell --bin tahoe-design-export -- \
//!     apple/Sources/TahoeDesign/TahoeDesign.swift
//!
//! # Check it is up to date, for CI. Exits 1 if it is stale.
//! cargo run -p tahoe-shell --bin tahoe-design-export -- --check \
//!     apple/Sources/TahoeDesign/TahoeDesign.swift
//! ```
//!
//! The Rust [`DesignSystem`] is the single source of truth. This binary is
//! the only supported way to produce the Swift.

use std::{path::PathBuf, process::ExitCode};

use tahoe_shell::design::DesignSystem;

const DEFAULT_OUTPUT: &str = "apple/Sources/TahoeDesign/TahoeDesign.swift";

const USAGE: &str = "\
tahoe-design-export — emit the SwiftUI copy of the Tahoe design system

USAGE:
    tahoe-design-export [--check] [--stdout] [<path>]

ARGS:
    <path>      Where to write. Defaults to apple/Sources/TahoeDesign/TahoeDesign.swift

OPTIONS:
    --check     Do not write; exit 1 if the file differs from what would be written
    --stdout    Print to standard output instead of writing
    --single    Emit only the dark appearance (for inspecting the emitter)
    -h, --help  Show this text

The Rust design system is the source of truth. Editing the generated Swift
by hand will fail the `swift_emitter` tests in tahoe-shell.
";

fn main() -> ExitCode {
    let mut check = false;
    let mut stdout = false;
    let mut single = false;
    let mut path: Option<PathBuf> = None;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--check" => check = true,
            "--stdout" => stdout = true,
            "--single" => single = true,
            other if other.starts_with('-') => {
                eprintln!("tahoe-design-export: unknown flag {other}\n\n{USAGE}");
                return ExitCode::from(2);
            }
            other => path = Some(PathBuf::from(other)),
        }
    }

    let dark = DesignSystem::dark();
    let light = DesignSystem::light();
    let rendered = if single {
        dark.to_swift()
    } else {
        dark.to_swift_with_appearances(&light)
    };

    if stdout {
        print!("{rendered}");
        return ExitCode::SUCCESS;
    }

    let path = path.unwrap_or_else(|| PathBuf::from(DEFAULT_OUTPUT));

    if check {
        let existing = std::fs::read_to_string(&path);
        return match existing {
            Ok(text) if text == rendered => {
                println!("{}: up to date", path.display());
                ExitCode::SUCCESS
            }
            Ok(_) => {
                eprintln!(
                    "{}: stale. Regenerate with:\n    cargo run -p tahoe-shell --bin tahoe-design-export -- {}",
                    path.display(),
                    path.display()
                );
                ExitCode::FAILURE
            }
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                ExitCode::FAILURE
            }
        };
    }

    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!("tahoe-design-export: create {}: {e}", parent.display());
        return ExitCode::FAILURE;
    }
    if let Err(e) = std::fs::write(&path, &rendered) {
        eprintln!("tahoe-design-export: write {}: {e}", path.display());
        return ExitCode::FAILURE;
    }
    println!(
        "{}: {} lines, {} bytes",
        path.display(),
        rendered.lines().count(),
        rendered.len()
    );
    ExitCode::SUCCESS
}
