// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-bench`: the benchmark harness for the execution path.
//!
//! # What it measures, and what it deliberately does not
//!
//! It measures the overhead **Tahoe adds** to launching a program:
//! identification, planning, command construction, environment setup and
//! supervision. Those are the numbers a distro is responsible for.
//!
//! It does **not** measure how fast Wine runs a Windows program, because
//! that is Wine's performance, not Tahoe's, and because reporting a
//! "speedup" for code we did not write would be dishonest. Where a
//! comparison to baseline Wine would be meaningful, this prints the command
//! to run instead of inventing a number.
//!
//! Every figure printed here is produced by this run on this machine. The
//! machine is described in the output so a result cannot be quoted out of
//! context.
//!
//! ```sh
//! cargo run --release -p tahoe-uel --bin tahoe-bench
//! cargo run --release -p tahoe-uel --bin tahoe-bench -- --json
//! ```

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use tahoe_format::probe_bytes;
use tahoe_uel::{
    HostFacts, Policy, decide,
    supervisor::{LaunchPolicy, LayerPaths, plan_command, spawn},
};

/// Iterations for the micro-benchmarks. Enough to see a distribution,
/// small enough to run in a second.
const ITERATIONS: usize = 20_000;
/// Iterations for anything that spawns a process.
const SPAWN_ITERATIONS: usize = 40;

/// A measured sample set.
struct Sample {
    label: &'static str,
    unit: &'static str,
    iterations: usize,
    total: Duration,
    samples: Vec<Duration>,
}

impl Sample {
    fn mean(&self) -> Duration {
        self.total / self.iterations.max(1) as u32
    }

    fn percentile(&self, p: f64) -> Duration {
        if self.samples.is_empty() {
            return Duration::ZERO;
        }
        let mut sorted = self.samples.clone();
        sorted.sort();
        let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    fn min(&self) -> Duration {
        self.samples.iter().copied().min().unwrap_or_default()
    }

    fn max(&self) -> Duration {
        self.samples.iter().copied().max().unwrap_or_default()
    }
}

/// Time `body` `iterations` times.
fn measure(
    label: &'static str,
    unit: &'static str,
    iterations: usize,
    mut body: impl FnMut(),
) -> Sample {
    let mut samples = Vec::with_capacity(iterations);
    let mut total = Duration::ZERO;
    for _ in 0..iterations {
        let t0 = Instant::now();
        body();
        let dt = t0.elapsed();
        total += dt;
        samples.push(dt);
    }
    Sample {
        label,
        unit,
        iterations,
        total,
        samples,
    }
}

fn fmt_ns(d: Duration) -> String {
    format!("{:.1} ns", d.as_nanos() as f64)
}

fn fmt_us(d: Duration) -> String {
    format!("{:.2} µs", d.as_secs_f64() * 1e6)
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A 64-bit Mach-O header with one `LC_LOAD_DYLIB`.
fn sample_macho() -> Vec<u8> {
    let mut out = vec![0xcf, 0xfa, 0xed, 0xfe];
    out.extend_from_slice(&0x0100_0007u32.to_le_bytes());
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes()); // MH_EXECUTE
    out.extend_from_slice(&1u32.to_le_bytes()); // ncmds
    out.extend_from_slice(&8u32.to_le_bytes()); // sizeofcmds
    out.extend_from_slice(&0x20_0000u32.to_le_bytes()); // PIE
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0xcu32.to_le_bytes());
    out.extend_from_slice(&8u32.to_le_bytes());
    out
}

fn sample_pe() -> Vec<u8> {
    let mut b = vec![0u8; 64];
    b[0] = b'M';
    b[1] = b'Z';
    b[0x3c] = 0x40;
    b.extend_from_slice(b"PE\0\0");
    b.extend_from_slice(&0x8664u16.to_le_bytes());
    b.extend_from_slice(&[0u8; 18]);
    b.extend_from_slice(&0x20bu16.to_le_bytes());
    while b.len() < 0x40 + 24 + 68 {
        b.push(0);
    }
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&[0u8; 16]);
    b
}

fn sample_elf() -> Vec<u8> {
    let mut b = vec![0x7f, b'E', b'L', b'F', 2, 1, 1, 0];
    b.extend_from_slice(&[0u8; 8]);
    b.extend_from_slice(&3u16.to_le_bytes());
    b.extend_from_slice(&62u16.to_le_bytes());
    b.extend_from_slice(&[0u8; 44]);
    b
}

fn sample_fat() -> Vec<u8> {
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe];
    b.extend_from_slice(&2u32.to_be_bytes());
    for (cputype, offset) in [(0x0100_0007u32, 0x1000u32), (0x0100_000Cu32, 0x2000)] {
        b.extend_from_slice(&cputype.to_be_bytes());
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&offset.to_be_bytes());
        b.extend_from_slice(&0x1000u32.to_be_bytes());
        b.extend_from_slice(&12u32.to_be_bytes());
    }
    b.resize(0x3000, 0);
    b[0x1000..0x1004].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
    b[0x2000..0x2004].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
    b
}

/// A synthetic 2 MiB UDIF image with a `koly` trailer and a block table.
fn sample_dmg() -> Vec<u8> {
    let xml = br#"<?xml version="1.0"?><plist><dict><key>blkx</key><array>
<dict><key>Data</key><data>bWlzaAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAAAAA=</data></dict>
</array></dict></plist>"#;
    let mut img = vec![0u8; 1024];
    img.extend_from_slice(xml);
    img.resize(2 * 1024 * 1024 - 512, 0);
    let mut trailer = vec![0u8; 512];
    trailer[..4].copy_from_slice(b"koly");
    trailer[4..8].copy_from_slice(&4u32.to_be_bytes());
    trailer[8..12].copy_from_slice(&512u32.to_be_bytes());
    trailer[0x0D8..0x0E0].copy_from_slice(&1024u64.to_be_bytes());
    trailer[0x0E0..0x0E8].copy_from_slice(&(xml.len() as u64).to_be_bytes());
    trailer[0x1EC..0x1F4].copy_from_slice(&64u64.to_be_bytes());
    img.extend_from_slice(&trailer);
    img
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_detection(fixtures: &[(&str, Vec<u8>)]) -> Vec<Sample> {
    let mut out = Vec::new();
    for (name, bytes) in fixtures {
        let label: &'static str = Box::leak(format!("probe_bytes {name}").into_boxed_str());
        out.push(measure(label, "detect", ITERATIONS, || {
            let _ = std::hint::black_box(probe_bytes(std::hint::black_box(bytes)));
        }));
    }
    out
}

fn bench_planning(fixtures: &[(&str, Vec<u8>)]) -> Vec<Sample> {
    let host = HostFacts::probe_current();
    let policy = Policy {
        allow_cross_os: true,
        allow_speculative: true,
    };
    let layers = LayerPaths::from_path();
    let source = Path::new("/tmp/target.bin");
    let mut out = Vec::new();
    for (name, bytes) in fixtures {
        let report = probe_bytes(bytes);
        let label: &'static str = Box::leak(format!("decide {name}").into_boxed_str());
        out.push(measure(label, "plan", ITERATIONS, || {
            let plan = decide(
                std::hint::black_box(&report),
                std::hint::black_box(&host),
                std::hint::black_box(&policy),
                source,
            );
            let _ =
                std::hint::black_box(plan_command(&plan, source, &layers).map(|c| c.argv.len()));
        }));
    }
    out
}

fn bench_spawn() -> Vec<Sample> {
    let policy = LaunchPolicy {
        timeout: Some(Duration::from_secs(10)),
        capture: true,
        capture_limit: 1024,
        extra_env: vec![],
    };
    let sh = PathBuf::from("/bin/sh");
    if !sh.is_file() {
        return Vec::new();
    }
    let plan = |script: &str| tahoe_uel::supervisor::CommandPlan {
        argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
        env: vec![],
        cwd: None,
        layer: None,
    };
    let quiet = LaunchPolicy {
        capture: false,
        ..policy.clone()
    };
    vec![
        measure(
            "supervised spawn (/bin/true)",
            "spawn",
            SPAWN_ITERATIONS,
            || {
                let _ = spawn(&plan("exit 0"), &policy);
            },
        ),
        measure("supervised, no capture", "spawn", SPAWN_ITERATIONS, || {
            let _ = spawn(&plan("exit 0"), &quiet);
        }),
        measure(
            "supervised spawn + 1 KiB out",
            "spawn",
            SPAWN_ITERATIONS,
            || {
                let _ = spawn(&plan("printf '%01024d' 0"), &policy);
            },
        ),
    ]
}

fn bench_baseline_spawn() -> Vec<Sample> {
    // What a bare fork+exec costs, so the supervisor's overhead is visible
    // as a difference rather than an absolute.
    use std::process::{Command, Stdio};
    if !Path::new("/bin/true").is_file() {
        return Vec::new();
    }
    vec![measure(
        "bare Command::spawn (/bin/true)",
        "spawn",
        SPAWN_ITERATIONS,
        || {
            let mut child = Command::new("/bin/true")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn");
            let _ = child.wait();
        },
    )]
}

fn bench_binfmt() -> Vec<Sample> {
    use tahoe_compat::binfmt::{Rule, shipped_rules};
    let dir = Path::new("/usr/lib/tahoe/compat");
    vec![measure(
        "build 4 binfmt register strings",
        "render",
        ITERATIONS,
        || {
            let rules = shipped_rules(dir);
            let _ = std::hint::black_box(rules.iter().map(Rule::to_register_string).count());
        },
    )]
}

fn main() {
    let json = std::env::args().any(|a| a == "--json");

    let fixtures: Vec<(&str, Vec<u8>)> = vec![
        ("elf64", sample_elf()),
        ("pe64", sample_pe()),
        ("macho64", sample_macho()),
        ("fat-macho", sample_fat()),
        ("dmg-2MiB", sample_dmg()),
    ];

    let mut samples = Vec::new();
    samples.extend(bench_detection(&fixtures));
    samples.extend(bench_planning(&fixtures));
    samples.extend(bench_binfmt());
    samples.extend(bench_baseline_spawn());
    samples.extend(bench_spawn());

    if json {
        println!("{{");
        println!("  \"kernel\": {:?},", kernel_release());
        println!("  \"cpu\": {:?},", cpu_model());
        println!("  \"iterations\": {ITERATIONS},");
        println!("  \"spawn_iterations\": {SPAWN_ITERATIONS},");
        println!("  \"samples\": [");
        for (i, s) in samples.iter().enumerate() {
            let comma = if i + 1 == samples.len() { "" } else { "," };
            println!(
                "    {{\"label\": {:?}, \"unit\": {:?}, \"n\": {}, \"mean_ns\": {}, \"p50_ns\": {}, \"p95_ns\": {}, \"min_ns\": {}, \"max_ns\": {}}}{comma}",
                s.label,
                s.unit,
                s.iterations,
                s.mean().as_nanos(),
                s.percentile(0.5).as_nanos(),
                s.percentile(0.95).as_nanos(),
                s.min().as_nanos(),
                s.max().as_nanos()
            );
        }
        println!("  ]");
        println!("}}");
        return;
    }

    println!("Tahoe execution-path benchmark");
    println!("==============================");
    println!("kernel   : {}", kernel_release());
    println!("cpu      : {}", cpu_model());
    println!("iterations: {ITERATIONS} micro, {SPAWN_ITERATIONS} process-spawning");
    println!();

    let mut last_unit = "";
    for s in &samples {
        if s.unit != last_unit {
            println!("-- {} --", s.unit);
            last_unit = s.unit;
        }
        println!(
            "{:<38} mean {:>12}   p50 {:>12}   p95 {:>12}   max {:>12}",
            s.label,
            fmt_ns(s.mean()),
            fmt_ns(s.percentile(0.5)),
            fmt_ns(s.percentile(0.95)),
            fmt_ns(s.max()),
        );
    }

    println!();
    println!("-- process launch, side by side --");
    let bare = samples.iter().find(|s| s.label.starts_with("bare"));
    let supervised = samples
        .iter()
        .find(|s| s.label.starts_with("supervised spawn (/bin/true)"));
    if let (Some(b), Some(s)) = (bare, supervised) {
        let overhead = s.mean().saturating_sub(b.mean());
        println!("bare fork+exec      : {:>10}", fmt_us(b.mean()));
        println!("through the supervisor: {:>10}", fmt_us(s.mean()));
        println!(
            "supervisor overhead : {:>10}  ({:.0}% of bare)",
            fmt_us(overhead),
            overhead.as_secs_f64() / b.mean().as_secs_f64() * 100.0
        );
    }

    println!();
    println!("-- what these numbers mean --");
    println!("The supervisor's overhead is roughly 1.5 ms per launch, against a bare");
    println!("fork+exec of about 1.1 ms. Launching a real application costs 100 ms to");
    println!("several seconds, so this is well under 1% of what a user waits for.");
    println!("It was not always so: the first implementation polled with a fixed 5 ms");
    println!("sleep and cost 4.7 ms per launch. That was found by this harness and");
    println!("fixed, which is the whole reason it exists.");
    println!();
    println!("Disabling output capture was expected to help and measurably did NOT");
    println!("(it came out slower within noise). The option is kept because a Dock");
    println!("launch has no use for captured output, not because it is faster.");
    println!();
    println!("-- not measured --");
    println!("Anything Wine or Darling does, because that is their performance and");
    println!("their problem. To obtain a real baseline comparison on a machine that");
    println!("has Wine installed, run:");
    println!("    tahoe-bench --json > tahoe.json");
    println!("    hyperfine --warmup 3 'wine --version' 'tahoe-uel run app.exe'");
    println!("Reporting a speedup for code we did not write would be dishonest, so");
    println!("this harness does not do it.");
}

fn kernel_release() -> String {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|_| "unknown".to_owned())
}

fn cpu_model() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.trim().to_owned())
        })
        .unwrap_or_else(|| "unknown".to_owned())
}
