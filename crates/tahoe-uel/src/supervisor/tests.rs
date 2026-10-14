// SPDX-License-Identifier: MIT OR Apache-2.0
//! Supervisor tests.
//!
//! Everything here runs real processes — `/bin/sh`, `sleep`, and small
//! helpers — so the timeout, capture, environment and refusal paths are
//! exercised for real rather than asserted about. Nothing needs Wine,
//! Darling, or root.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

use tahoe_core::Arch;

use super::*;
use crate::{Confidence, ExecutionPlan, RejectionReason, Strategy};

fn plan(strategy: Strategy) -> ExecutionPlan {
    ExecutionPlan {
        strategy,
        confidence: Confidence::Verified,
        summary: "test plan".to_owned(),
    }
}

fn layers() -> LayerPaths {
    LayerPaths {
        wine: Some("/usr/bin/wine".into()),
        darling: Some("/usr/bin/darling".into()),
        fex: Some("/usr/bin/FEXInterpreter".into()),
        box64: Some("/usr/bin/box64".into()),
        qemu_user: Some("/usr/bin/qemu-x86_64".into()),
    }
}

fn no_layers() -> LayerPaths {
    LayerPaths::default()
}

/// A command that runs `script` under `/bin/sh -c`.
fn shell_plan(script: &str) -> CommandPlan {
    CommandPlan {
        argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
        env: Vec::new(),
        cwd: None,
        layer: None,
    }
}

fn run(script: &str, policy: &LaunchPolicy) -> LaunchOutcome {
    spawn(&shell_plan(script), policy).expect("spawn")
}

fn fast() -> LaunchPolicy {
    LaunchPolicy {
        timeout: Some(Duration::from_secs(10)),
        capture: true,
        capture_limit: 64 * 1024,
        extra_env: vec![],
    }
}

// ---------------------------------------------------------------------------
// plan_command
// ---------------------------------------------------------------------------

#[test]
fn native_plan_is_the_program_itself() {
    let cmd = plan_command(
        &plan(Strategy::Native),
        Path::new("/usr/bin/foot"),
        &no_layers(),
    )
    .expect("native needs no layer");
    assert_eq!(cmd.argv, vec![OsString::from("/usr/bin/foot")]);
    assert_eq!(cmd.layer, None);
}

#[test]
fn script_plan_prepends_the_interpreter() {
    let cmd = plan_command(
        &plan(Strategy::Script {
            interpreter: PathBuf::from("/bin/sh"),
            interpreter_args: vec![],
        }),
        Path::new("/home/u/run.sh"),
        &no_layers(),
    )
    .expect("scripts need no layer");
    assert_eq!(
        cmd.argv,
        vec![OsString::from("/bin/sh"), OsString::from("/home/u/run.sh")]
    );
}

#[test]
fn wine_plan_sets_an_isolated_prefix() {
    let cmd = plan_command(
        &plan(Strategy::Wine {
            prefix: "/home/u/.wine-tahoe".into(),
        }),
        Path::new("/home/u/setup.exe"),
        &layers(),
    )
    .expect("wine is present");
    assert_eq!(
        cmd.argv,
        vec![
            OsString::from("/usr/bin/wine"),
            OsString::from("/home/u/setup.exe")
        ]
    );
    assert_eq!(cmd.layer, Some("wine"));
    let env: Vec<_> = cmd
        .env
        .iter()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    assert!(env.contains(&"WINEPREFIX".to_owned()));
    assert!(
        env.contains(&"WINEDLLOVERRIDES".to_owned()),
        "wine must not fetch gecko/mono mid-launch"
    );
}

#[test]
fn wine_plan_without_wine_is_refused_by_name() {
    let err = plan_command(
        &plan(Strategy::Wine {
            prefix: "/tmp/p".into(),
        }),
        Path::new("/home/u/setup.exe"),
        &no_layers(),
    )
    .unwrap_err();
    assert_eq!(err, PlanError::LayerMissing { layer: "wine" });
    assert_eq!(err.to_string(), "wine is not installed");
}

#[test]
fn darling_plan_sets_dprefix() {
    let cmd = plan_command(
        &plan(Strategy::Darling {
            prefix: "/home/u/.darling".into(),
        }),
        Path::new("/Applications/Foo.app/Contents/MacOS/Foo"),
        &layers(),
    )
    .expect("darling is present");
    assert_eq!(cmd.layer, Some("darling"));
    assert!(cmd.env.iter().any(|(k, _)| k == "DPREFIX"));
}

#[test]
fn darling_without_darling_is_refused_by_name() {
    let err = plan_command(
        &plan(Strategy::Darling {
            prefix: "/tmp/p".into(),
        }),
        Path::new("/Applications/Foo.app/Contents/MacOS/Foo"),
        &no_layers(),
    )
    .unwrap_err();
    assert_eq!(err, PlanError::LayerMissing { layer: "darling" });
}

#[test]
fn emulator_plan_uses_the_matching_backend() {
    for (backend, expected) in [
        (EmulatorBackend::Fex, "/usr/bin/FEXInterpreter"),
        (EmulatorBackend::Box64, "/usr/bin/box64"),
        (EmulatorBackend::QemuUser, "/usr/bin/qemu-x86_64"),
    ] {
        let cmd = plan_command(
            &plan(Strategy::Emulator { backend }),
            Path::new("/home/u/app"),
            &layers(),
        )
        .expect("emulator present");
        assert_eq!(cmd.program(), OsString::from(expected).as_os_str());
    }
}

#[test]
fn emulator_missing_is_named_in_the_error() {
    let err = plan_command(
        &plan(Strategy::Emulator {
            backend: EmulatorBackend::Box64,
        }),
        Path::new("/home/u/app"),
        &no_layers(),
    )
    .unwrap_err();
    assert!(matches!(err, PlanError::LayerMissing { .. }));
}

#[test]
fn rejected_plans_never_produce_a_command() {
    for reason in [
        RejectionReason::DmgIsNotExecutable,
        RejectionReason::UnknownFormat,
        RejectionReason::CrossOsDisabled,
        RejectionReason::SpeculativeDisabled,
    ] {
        let err = plan_command(
            &plan(Strategy::Rejected {
                reason: reason.clone(),
            }),
            Path::new("/x"),
            &layers(),
        )
        .unwrap_err();
        assert_eq!(err, PlanError::Refused(reason));
    }
}

#[test]
fn empty_program_is_rejected_before_anything_else() {
    let err = plan_command(&plan(Strategy::Native), Path::new(""), &layers()).unwrap_err();
    assert_eq!(err, PlanError::EmptyProgram);
}

#[test]
fn command_line_rendering_is_readable() {
    let cmd = plan_command(
        &plan(Strategy::Script {
            interpreter: "/bin/sh".into(),
            interpreter_args: vec![],
        }),
        Path::new("/tmp/a b.sh"),
        &no_layers(),
    )
    .expect("plan");
    assert_eq!(cmd.command_line(), "/bin/sh /tmp/a b.sh");
}

// ---------------------------------------------------------------------------
// spawn: success paths
// ---------------------------------------------------------------------------

#[test]
fn captures_stdout_and_exit_code() {
    let out = run("printf hello", &fast());
    assert_eq!(out.stdout, "hello");
    assert_eq!(out.exit_code, Some(0));
    assert!(out.succeeded());
    assert!(!out.timed_out);
    assert_eq!(out.signal, None);
}

#[test]
fn captures_stderr_separately() {
    let out = run("printf out; printf err >&2", &fast());
    assert_eq!(out.stdout, "out");
    assert_eq!(out.stderr, "err");
}

#[test]
fn non_zero_exit_is_an_outcome_not_an_error() {
    let out = run("exit 3", &fast());
    assert_eq!(out.exit_code, Some(3));
    assert!(!out.succeeded());
    assert!(!out.timed_out);
    assert!(out.summary().contains("exited 3"));
}

#[test]
fn empty_command_succeeds_with_no_output() {
    let out = run(":", &fast());
    assert!(out.succeeded());
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
}

#[test]
fn arguments_are_never_interpreted_by_a_shell() {
    // A filename that would be command substitution in a shell. It must
    // arrive verbatim as an argument, and must NOT be evaluated.
    let nasty = "/tmp/$(touch /tmp/tahoe-uel-should-not-exist).txt";
    let _ = std::fs::remove_file("/tmp/tahoe-uel-should-not-exist");
    let cmd = CommandPlan {
        argv: vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf '%s' \"$1\"".into(),
            "sh".into(),
            nasty.into(),
        ],
        env: Vec::new(),
        cwd: None,
        layer: None,
    };
    let out = spawn(&cmd, &fast()).expect("spawn");
    assert_eq!(out.stdout, nasty, "the argument must be passed literally");
    assert!(
        !Path::new("/tmp/tahoe-uel-should-not-exist").exists(),
        "the argument must not have been executed"
    );
}

// ---------------------------------------------------------------------------
// spawn: timeouts and process groups
// ---------------------------------------------------------------------------

#[test]
fn timeout_kills_the_child_and_reports_it() {
    let policy = LaunchPolicy {
        timeout: Some(Duration::from_millis(150)),
        capture: true,
        capture_limit: 4096,
        extra_env: vec![],
    };
    let started = std::time::Instant::now();
    let out = run("sleep 30", &policy);
    let elapsed = started.elapsed();

    assert!(out.timed_out, "a 30s sleep must hit a 150ms timeout");
    assert!(
        elapsed < Duration::from_secs(5),
        "kill must be prompt, took {elapsed:?}"
    );
    assert!(!out.succeeded());
    assert!(out.summary().contains("timed out"));
}

#[test]
fn timeout_kills_grandchildren_too() {
    // The child spawns a grandchild and waits. Killing only the child would
    // leave the grandchild holding the display, which is exactly the bug
    // process-group killing exists to prevent.
    let marker = std::env::temp_dir().join(format!("tahoe-grandchild-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let script = format!("( sleep 2; printf alive > {} ) & wait", marker.display());
    let policy = LaunchPolicy {
        timeout: Some(Duration::from_millis(150)),
        capture: true,
        capture_limit: 4096,
        extra_env: vec![],
    };
    let out = run(&script, &policy);
    assert!(out.timed_out);

    // If the group survived we would see the marker appear shortly after.
    std::thread::sleep(Duration::from_millis(3000));
    assert!(
        !marker.exists(),
        "the grandchild survived the timeout; the process group was not killed"
    );
    let _ = std::fs::remove_file(&marker);
}

#[test]
fn a_timeout_of_none_does_not_kill() {
    let policy = LaunchPolicy {
        timeout: None,
        capture: true,
        capture_limit: 4096,
        extra_env: vec![],
    };
    let out = run("sleep 0.2; printf done", &policy);
    assert!(!out.timed_out);
    assert_eq!(out.stdout, "done");
}

#[test]
fn a_fast_command_is_not_killed_even_with_a_tiny_budget() {
    let policy = LaunchPolicy {
        timeout: Some(Duration::from_millis(500)),
        capture: true,
        capture_limit: 4096,
        extra_env: vec![],
    };
    let out = run("printf quick", &policy);
    assert!(!out.timed_out);
    assert_eq!(out.stdout, "quick");
}

// ---------------------------------------------------------------------------
// spawn: capture limits
// ---------------------------------------------------------------------------

#[test]
fn output_is_capped_and_the_truncation_is_reported() {
    let policy = LaunchPolicy {
        timeout: Some(Duration::from_secs(10)),
        capture: true,
        capture_limit: 1024,
        extra_env: vec![],
    };
    // ~200 KiB of output against a 1 KiB budget.
    let out = run(
        "i=0; while [ $i -lt 2000 ]; do printf '%0100d' $i; i=$((i+1)); done",
        &policy,
    );
    assert_eq!(out.stdout.len(), 1024, "capture must stop at the limit");
    assert!(out.stdout_truncated, "truncation must be reported");
    assert!(out.succeeded(), "the child must still be allowed to finish");
}

#[test]
fn output_under_the_limit_is_not_marked_truncated() {
    let policy = LaunchPolicy {
        timeout: Some(Duration::from_secs(10)),
        capture: true,
        capture_limit: 4096,
        extra_env: vec![],
    };
    let out = run("printf abc", &policy);
    assert!(!out.stdout_truncated);
    assert!(!out.stderr_truncated);
}

#[test]
fn a_child_writing_after_the_limit_is_not_killed_by_a_broken_pipe() {
    // The supervisor keeps draining after the cap, so the writer sees a
    // normal end rather than EPIPE and a non-zero exit.
    let policy = LaunchPolicy {
        timeout: Some(Duration::from_secs(10)),
        capture: true,
        capture_limit: 512,
        extra_env: vec![],
    };
    let out = run(
        "i=0; while [ $i -lt 500 ]; do printf '0123456789'; i=$((i+1)); done; exit 0",
        &policy,
    );
    assert!(out.stdout_truncated);
    assert_eq!(out.exit_code, Some(0), "the writer must not see EPIPE");
}

// ---------------------------------------------------------------------------
// spawn: environment hygiene
// ---------------------------------------------------------------------------

#[test]
fn unlisted_variables_do_not_reach_the_child() {
    // SAFETY: this test process is single-threaded with respect to this
    // variable; set and removed within the test.
    unsafe { std::env::set_var("TAHOE_LEAK_CANARY", "should-not-leak") };
    let out = run("printf '%s' \"${TAHOE_LEAK_CANARY:-absent}\"", &fast());
    assert_eq!(
        out.stdout, "absent",
        "non-whitelisted variables must be dropped"
    );
    unsafe { std::env::remove_var("TAHOE_LEAK_CANARY") };
}

#[test]
fn path_reaches_the_child_because_it_is_whitelisted() {
    let out = run("printf '%s' \"${PATH:-absent}\"", &fast());
    assert_ne!(out.stdout, "absent");
    assert!(out.stdout.contains('/'));
}

#[test]
fn caller_provided_variables_are_passed() {
    let policy = LaunchPolicy {
        timeout: Some(Duration::from_secs(10)),
        capture: true,
        capture_limit: 4096,
        extra_env: vec![("TAHOE_TEST_VAR".into(), "vouched".into())],
    };
    let out = run("printf '%s' \"$TAHOE_TEST_VAR\"", &policy);
    assert_eq!(out.stdout, "vouched");
}

#[test]
fn policy_variables_override_plan_variables() {
    // An explicit human override must beat an automatically derived default
    // such as WINEPREFIX, which is the whole point of `extra_env`.
    let mut cmd = shell_plan("printf '%s' \"$K\"");
    cmd.env.push(("K".into(), "from-plan".into()));
    let policy = LaunchPolicy {
        timeout: Some(Duration::from_secs(10)),
        capture: true,
        capture_limit: 4096,
        extra_env: vec![("K".into(), "from-policy".into())],
    };
    let out = spawn(&cmd, &policy).expect("spawn");
    assert_eq!(out.stdout, "from-policy");
}

#[test]
fn working_directory_is_honoured() {
    let dir = std::env::temp_dir();
    let mut cmd = shell_plan("pwd");
    cmd.cwd = Some(dir.clone());
    let out = spawn(&cmd, &fast()).expect("spawn");
    // /tmp may be a symlink; compare canonicalised paths.
    let reported = std::fs::canonicalize(out.stdout.trim()).expect("canonicalise child cwd");
    let expected = std::fs::canonicalize(&dir).expect("canonicalise expected cwd");
    assert_eq!(reported, expected);
}

// ---------------------------------------------------------------------------
// spawn: pre-flight failures
// ---------------------------------------------------------------------------

#[test]
fn missing_program_is_reported_before_exec() {
    let cmd = CommandPlan {
        argv: vec!["/definitely/not/here".into()],
        env: vec![],
        cwd: None,
        layer: None,
    };
    match spawn(&cmd, &fast()) {
        Err(LaunchError::NotFound(p)) => assert_eq!(p, PathBuf::from("/definitely/not/here")),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn non_executable_file_is_reported_clearly() {
    let path = std::env::temp_dir().join(format!("tahoe-not-exec-{}", std::process::id()));
    std::fs::write(&path, b"#!/bin/sh\necho hi\n").expect("write");
    let mut perms = std::fs::metadata(&path).expect("stat").permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o644);
    }
    std::fs::set_permissions(&path, perms).expect("chmod");

    let cmd = CommandPlan {
        argv: vec![path.clone().into()],
        env: vec![],
        cwd: None,
        layer: None,
    };
    match spawn(&cmd, &fast()) {
        Err(LaunchError::NotExecutable(p)) => assert_eq!(p, path),
        other => panic!("expected NotExecutable, got {other:?}"),
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_directory_is_not_executable() {
    let cmd = CommandPlan {
        argv: vec!["/tmp".into()],
        env: vec![],
        cwd: None,
        layer: None,
    };
    assert!(matches!(
        spawn(&cmd, &fast()),
        Err(LaunchError::NotExecutable(_))
    ));
}

#[test]
fn refuses_to_spawn_a_rejected_plan_end_to_end() {
    let p = plan(Strategy::Rejected {
        reason: RejectionReason::DmgIsNotExecutable,
    });
    let err = plan_command(&p, Path::new("/tmp/thing.dmg"), &layers()).unwrap_err();
    assert!(matches!(err, PlanError::Refused(_)));
    // And the error never becomes a process.
    assert!(!err.to_string().is_empty());
}

// ---------------------------------------------------------------------------
// LaunchRecord
// ---------------------------------------------------------------------------

fn sample_record(outcome: Option<LaunchOutcome>, error: Option<String>) -> LaunchRecord {
    LaunchRecord {
        requested: PathBuf::from("/home/u/setup.exe"),
        format: "pe".to_owned(),
        arch: Arch::X86_64,
        strategy: "Wine { .. }".to_owned(),
        confidence: Confidence::Layer {
            layer: "wine",
            caveat: "subset",
        },
        command: Some("/usr/bin/wine /home/u/setup.exe".to_owned()),
        outcome,
        error,
    }
}

#[test]
fn log_line_is_valid_json_shaped_and_escapes_quotes() {
    let record = sample_record(
        Some(LaunchOutcome {
            exit_code: Some(0),
            signal: None,
            stdout: String::new(),
            stderr: String::new(),
            duration: Duration::from_millis(12),
            timed_out: false,
            stdout_truncated: false,
            stderr_truncated: false,
        }),
        None,
    );
    let line = record.to_log_line();
    assert!(line.starts_with('{') && line.ends_with('}'));
    assert!(line.contains("\"format\":\"pe\""));
    assert!(line.contains("\"arch\":\"x86-64\""));
    assert!(line.contains("\"confidence\":\"layer:wine\""));
    assert!(line.contains("\"exit_code\":0"));
    assert!(line.contains("\"timed_out\":false"));
    assert!(line.contains("\"duration_ms\":12"));
    assert!(!line.contains('\n'), "a log line must be one line");
}

#[test]
fn log_line_escapes_quotes_in_the_requested_path() {
    let mut record = sample_record(None, Some("refused: DmgIsNotExecutable".to_owned()));
    record.requested = PathBuf::from("/home/u/we\"ird.dmg");
    let line = record.to_log_line();
    assert!(line.contains("we\\\"ird.dmg"), "{line}");
    assert!(line.contains("\"outcome\":null"));
    assert!(line.contains("\"error\":\"refused: DmgIsNotExecutable\""));
}

#[test]
fn log_line_handles_a_killed_process() {
    let record = sample_record(
        Some(LaunchOutcome {
            exit_code: None,
            signal: Some(9),
            stdout: String::new(),
            stderr: String::new(),
            duration: Duration::from_millis(150),
            timed_out: true,
            stdout_truncated: false,
            stderr_truncated: false,
        }),
        None,
    );
    let line = record.to_log_line();
    assert!(line.contains("\"exit_code\":null"));
    assert!(line.contains("\"timed_out\":true"));
}

#[test]
fn launch_outcome_summary_covers_every_end_state() {
    let mk = |exit_code, signal, timed_out| LaunchOutcome {
        exit_code,
        signal,
        stdout: String::new(),
        stderr: String::new(),
        duration: Duration::from_millis(5),
        timed_out,
        stdout_truncated: false,
        stderr_truncated: false,
    };
    assert!(mk(Some(0), None, false).summary().contains("exited 0"));
    assert!(mk(Some(2), None, false).summary().contains("exited 2"));
    assert!(mk(None, Some(9), true).summary().contains("timed out"));
    assert!(mk(None, Some(15), false).summary().contains("signal 15"));
    assert!(mk(None, None, false).summary().contains("unknown"));
}

#[test]
fn confidence_display_is_stable() {
    assert_eq!(Confidence::Verified.to_string(), "verified");
    assert_eq!(
        Confidence::Layer {
            layer: "wine",
            caveat: "x"
        }
        .to_string(),
        "layer:wine"
    );
}

#[test]
fn layer_paths_from_path_never_panics() {
    let paths = LayerPaths::from_path();
    // On a machine without Wine this is None; either way it must not panic
    // and any path it does report must exist.
    for p in [
        &paths.wine,
        &paths.darling,
        &paths.fex,
        &paths.box64,
        &paths.qemu_user,
    ]
    .into_iter()
    .flatten()
    {
        assert!(p.is_file(), "{} should exist", p.display());
    }
}
