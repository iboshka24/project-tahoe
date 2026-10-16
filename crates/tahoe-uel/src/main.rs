// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-uel` CLI: identify a file, decide how to run it, and (on request)
//! supervise it.
//!
//! ```text
//! tahoe-uel host                 what this machine can actually run
//! tahoe-uel probe <path>...      identify and plan, run nothing
//! tahoe-uel plan  <path>...      same, as one log line per file
//! tahoe-uel run   <path> [args]  plan, then supervise; prints a launch record
//! ```
//!
//! `run` is the only subcommand that executes anything, and it refuses
//! anything the UEL does not have a strategy for. DMGs are refused outright.

use std::{path::PathBuf, time::Duration};

use tahoe_format::probe_file;
use tahoe_uel::{
    HostFacts, Policy, decide,
    supervisor::{LaunchPolicy, LaunchRecord, LayerPaths, PlanError, plan_command, spawn},
};

const USAGE: &str = "\
tahoe-uel — Universal Execution Layer

USAGE:
    tahoe-uel host
    tahoe-uel probe [--allow-speculative] [--no-cross-os] <path>...
    tahoe-uel plan  [--allow-speculative] [--no-cross-os] <path>...
    tahoe-uel run   [--allow-speculative] [--no-cross-os]
                    [--timeout <seconds>] [--capture-limit <bytes>]
                    <path> [args...]

NOTES:
    `probe` and `plan` never execute anything.
    `run` refuses any file the UEL has no strategy for, and never uses a shell.
";

fn usage() -> ! {
    eprintln!("{USAGE}");
    std::process::exit(2);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(cmd) = args.next() else { usage() };
    let rest: Vec<String> = args.collect();
    let exit = match cmd.as_str() {
        "host" => {
            cmd_host();
            0
        }
        "probe" => cmd_probe(&rest, false),
        "plan" => cmd_probe(&rest, true),
        "run" => cmd_run(&rest),
        "-h" | "--help" => {
            println!("{USAGE}");
            0
        }
        other => {
            eprintln!("unknown subcommand: {other}\n\n{USAGE}");
            2
        }
    };
    std::process::exit(exit);
}

fn cmd_host() {
    let host = HostFacts::probe_current();
    let layers = LayerPaths::from_path();
    println!("host_arch={}", host.host_arch);
    println!("wine={}", host.wine_available);
    println!("darling={}", host.darling_available);
    println!("emulator={}", host.emulator_available);
    println!("flatpak={}", host.flatpak_available);
    println!(
        "wine_path={}",
        layers
            .wine
            .map_or("-".to_owned(), |p| p.display().to_string())
    );
    println!(
        "umu_run={}",
        host.umu_available
            .as_ref()
            .map_or("-".to_owned(), |p| p.display().to_string())
    );
    println!(
        "proton_build={}",
        host.proton_build
            .as_ref()
            .map_or("-".to_owned(), |p| p.display().to_string())
    );
    // Every build found, not only the preferred one: when the choice looks
    // wrong the user needs to see what was on offer.
    for build in tahoe_compat::proton::ProtonInstall::discover() {
        println!(
            "proton_candidate={} flavour={:?}",
            build.root.display(),
            build.flavour
        );
    }
    println!(
        "darling_path={}",
        layers
            .darling
            .map_or("-".to_owned(), |p| p.display().to_string())
    );
}

/// Parse shared flags. Returns the policy and the positional arguments.
fn parse_common(rest: &[String]) -> Result<(Policy, Vec<String>), String> {
    let mut policy = Policy::default();
    let mut positional = Vec::new();
    for arg in rest {
        match arg.as_str() {
            "--allow-speculative" => policy.allow_speculative = true,
            "--no-cross-os" => policy.allow_cross_os = false,
            a if a.starts_with('-') && a != "-" => {
                return Err(format!("unknown flag: {a}"));
            }
            a => positional.push(a.to_owned()),
        }
    }
    Ok((policy, positional))
}

fn cmd_probe(rest: &[String], one_line: bool) -> i32 {
    let (policy, paths) = match parse_common(rest) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}\n\n{USAGE}");
            return 2;
        }
    };
    if paths.is_empty() {
        usage();
    }
    let host = HostFacts::probe_current();
    let layers = LayerPaths::from_path();
    let mut failed = false;
    for path in paths {
        let path = PathBuf::from(path);
        let report = match probe_file(&path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                failed = true;
                continue;
            }
        };
        let plan = decide(&report, &host, &policy, &path);
        if one_line {
            let command = plan_command(&plan, &path, &layers)
                .map(|c| c.command_line())
                .unwrap_or_else(|e| format!("<no command: {e}>"));
            let record = LaunchRecord {
                requested: path.clone(),
                format: report.format.to_string(),
                arch: report.arch,
                strategy: format!("{:?}", plan.strategy),
                confidence: plan.confidence,
                command: Some(command),
                outcome: None,
                error: None,
            };
            println!("{}", record.to_log_line());
        } else {
            println!(
                "{}: format={} arch={}",
                path.display(),
                report.format,
                report.arch
            );
            println!("  detail={:?}", report.detail);
            println!("  strategy={:?}", plan.strategy);
            println!("  confidence={}", plan.confidence);
            println!("  summary={}", plan.summary);
            match plan_command(&plan, &path, &layers) {
                Ok(cmd) => println!("  command={}", cmd.command_line()),
                Err(e) => println!("  command=<none: {e}>"),
            }
        }
    }
    i32::from(failed)
}

fn parse_run_args(rest: &[String]) -> Result<(Policy, LaunchPolicy, PathBuf, Vec<String>), String> {
    let mut policy = Policy::default();
    let mut launch = LaunchPolicy::default();
    let mut positional: Vec<String> = Vec::new();
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--allow-speculative" => policy.allow_speculative = true,
            "--no-cross-os" => policy.allow_cross_os = false,
            "--timeout" => {
                let v = it.next().ok_or("--timeout needs a value")?;
                let secs: f64 = v.parse().map_err(|_| format!("bad seconds: {v}"))?;
                if !secs.is_finite() || secs < 0.0 {
                    return Err(format!("bad seconds: {v}"));
                }
                launch.timeout = Some(Duration::from_secs_f64(secs));
            }
            "--no-timeout" => launch.timeout = None,
            "--capture-limit" => {
                let v = it.next().ok_or("--capture-limit needs a value")?;
                launch.capture_limit = v.parse().map_err(|_| format!("bad byte count: {v}"))?;
            }
            "--set" => {
                let v = it.next().ok_or("--set needs KEY=VALUE")?;
                let (k, val) = v
                    .split_once('=')
                    .ok_or_else(|| format!("--set needs KEY=VALUE, got {v}"))?;
                launch.extra_env.push((k.into(), val.into()));
            }
            a if a.starts_with('-') && a != "-" => return Err(format!("unknown flag: {a}")),
            a => positional.push(a.to_owned()),
        }
    }
    let Some(first) = positional.first() else {
        return Err("run needs a program path".to_owned());
    };
    let program = PathBuf::from(first);
    let args = positional.into_iter().skip(1).collect();
    Ok((policy, launch, program, args))
}

fn cmd_run(rest: &[String]) -> i32 {
    let (policy, launch_policy, path, extra_args) = match parse_run_args(rest) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}\n\n{USAGE}");
            return 2;
        }
    };

    let host = HostFacts::probe_current();
    let layers = LayerPaths::from_path();

    let report = match probe_file(&path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return 1;
        }
    };
    let plan = decide(&report, &host, &policy, &path);

    let mut command_plan = match plan_command(&plan, &path, &layers) {
        Ok(c) => c,
        Err(e) => {
            let record = LaunchRecord {
                requested: path.clone(),
                format: report.format.to_string(),
                arch: report.arch,
                strategy: format!("{:?}", plan.strategy),
                confidence: plan.confidence.clone(),
                command: None,
                outcome: None,
                error: Some(e.to_string()),
            };
            println!("{}", record.to_log_line());
            match e {
                PlanError::Refused(_) => {
                    eprintln!("tahoe-uel: refusing to run {}: {e}", path.display());
                    eprintln!("tahoe-uel: {}", plan.summary);
                }
                other => eprintln!("tahoe-uel: cannot run {}: {other}", path.display()),
            }
            return 1;
        }
    };
    command_plan
        .argv
        .extend(extra_args.into_iter().map(Into::into));

    let outcome = spawn(&command_plan, &launch_policy);
    let (outcome, error) = match outcome {
        Ok(o) => (Some(o), None),
        Err(e) => (None, Some(e.to_string())),
    };

    let record = LaunchRecord {
        requested: path.clone(),
        format: report.format.to_string(),
        arch: report.arch,
        strategy: format!("{:?}", plan.strategy),
        confidence: plan.confidence,
        command: Some(command_plan.command_line()),
        outcome: outcome.clone(),
        error: error.clone(),
    };
    println!("{}", record.to_log_line());

    match (outcome, error) {
        (Some(o), _) => {
            use std::io::Write;
            let _ = std::io::stdout().write_all(o.stdout.as_bytes());
            let _ = std::io::stderr().write_all(o.stderr.as_bytes());
            if let Some(code) = o.exit_code {
                return code.clamp(0, 255);
            }
            128
        }
        (None, Some(e)) => {
            eprintln!("tahoe-uel: {e}");
            1
        }
        (None, None) => 1,
    }
}
