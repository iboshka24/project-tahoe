// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-uel` CLI: probe a file, print the report and the execution plan.
//!
//! Read-only: never executes the probed file.

use std::path::PathBuf;
use tahoe_format::probe_file;
use tahoe_uel::{HostFacts, Policy, decide};

fn usage() -> ! {
    eprintln!("usage: tahoe-uel probe [--allow-speculative] <path>...");
    eprintln!("       tahoe-uel host");
    std::process::exit(2);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(cmd) = args.next() else { usage() };
    match cmd.as_str() {
        "host" => {
            let host = HostFacts::probe_current();
            println!("host_arch={}", host.host_arch);
            println!("wine={}", host.wine_available);
            println!("darling={}", host.darling_available);
            println!("emulator={}", host.emulator_available);
            println!("flatpak={}", host.flatpak_available);
        }
        "probe" => {
            let mut policy = Policy::default();
            let mut paths: Vec<PathBuf> = Vec::new();
            for arg in args {
                if arg == "--allow-speculative" {
                    policy.allow_speculative = true;
                } else if arg == "--no-cross-os" {
                    policy.allow_cross_os = false;
                } else if arg.starts_with('-') {
                    eprintln!("unknown flag: {arg}");
                    usage();
                } else {
                    paths.push(PathBuf::from(arg));
                }
            }
            if paths.is_empty() {
                usage();
            }
            let host = HostFacts::probe_current();
            let mut failed = false;
            for path in &paths {
                match probe_file(path) {
                    Ok(report) => {
                        let plan = decide(&report, &host, &policy, path);
                        println!(
                            "{}: format={} arch={}",
                            path.display(),
                            report.format,
                            report.arch
                        );
                        println!("  detail={:?}", report.detail);
                        println!("  strategy={:?}", plan.strategy);
                        println!("  confidence={:?}", plan.confidence);
                        println!("  summary={}", plan.summary);
                    }
                    Err(e) => {
                        eprintln!("{}: error: {e}", path.display());
                        failed = true;
                    }
                }
            }
            if failed {
                std::process::exit(1);
            }
        }
        _ => usage(),
    }
}
