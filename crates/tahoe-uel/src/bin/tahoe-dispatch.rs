// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-dispatch`: the interpreter `binfmt_misc` runs for foreign formats.
//!
//! # Why this is the closest thing to "native" that exists
//!
//! When you double-click a `.exe`, some process has to end up running Wine.
//! The only question is **how many processes are in the chain**:
//!
//! ```text
//! naive:      shell → wrapper → supervisor thread → wine → the program     (4)
//! this:       kernel → tahoe-dispatch → wine → the program                 (2)
//! ```
//!
//! `tahoe-dispatch` calls `execv` on Wine, which **replaces** its own process
//! image. It does not fork, does not stay resident, and does not appear in
//! the process tree. After the `execv` the chain is exactly the one a
//! shebang script would produce: the kernel's binfmt handler and the program.
//! There is no mechanism in Linux that removes the remaining hop, because
//! the `execve` that starts Wine has to come from somewhere.
//!
//! This is what `binfmt_misc` exists for, and it is why registering a rule
//! is better than wrapping the call in a launcher: with a rule,
//! `./installer.exe` in a shell, `xdg-open`, and a double-click all take the
//! same two-hop path.
//!
//! # It does not trust the magic that got it here
//!
//! `binfmt_misc` matched `MZ`, or `cf fa ed fe`, or `ca fe ba be`. Those are
//! byte patterns, and `ca fe ba be` is also a Java class file. So this
//! program re-identifies the file with the real parser and refuses if the
//! answer is not what the rule expected — a wrong dispatch is worse than no
//! dispatch, because it produces a confusing error from the wrong program.
//!
//! # Usage
//!
//! As a `binfmt_misc` interpreter:
//!
//! ```text
//! :tahoe-wine-pe:M::MZ::/usr/lib/tahoe/tahoe-dispatch:P
//! ```
//!
//! With the `P` flag the kernel calls it as
//! `tahoe-dispatch <binary> <original-argv0> [args...]`, so the first
//! argument is the file and the second is a courtesy copy of what the user
//! typed. This program skips the second and forwards the rest.
//!
//! # Environment
//!
//! Reaching this program means the user already asked to run the file, so
//! the UEL decides with `allow_speculative` off: a speculative launch should
//! be an explicit choice, not a side effect of typing a filename.

use std::{
    env,
    ffi::OsString,
    io::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

use tahoe_uel::{
    HostFacts, Policy, Strategy, decide, supervisor::LayerPaths, supervisor::plan_command,
};

/// Exit code used when the UEL refuses. 126 is "found but not executable",
/// which is what a shell reports for a file it cannot run.
const EXIT_REFUSED: i32 = 126;

fn main() {
    let args: Vec<OsString> = env::args_os().collect();
    let code = run(&args);
    std::process::exit(code);
}

fn run(args: &[OsString]) -> i32 {
    // `args[0]` is our own path. With `P`, `args[1]` is the binary and
    // `args[2]` is the original argv[0]; without it, `args[1]` is the binary
    // and there is no extra argument. Tell the two apart by checking whether
    // the second argument looks like the same path, which is what the kernel
    // supplies.
    let Some(first) = args.get(1) else {
        fail(
            "tahoe-dispatch: no file to run",
            "this program is a binfmt interpreter; see tahoe-compat binfmt rules",
        );
        return EXIT_REFUSED;
    };
    let binary = PathBuf::from(first);
    let rest: Vec<OsString> = if args.len() > 2 {
        let second = PathBuf::from(&args[2]);
        // With `P` the kernel appends the original argv[0]. It is the same
        // file, so skipping it is safe; when it is a different value the
        // user passed an argument and it must be forwarded.
        let is_argv0_echo = second == binary
            || second.file_name() == binary.file_name()
                && args.len() > 2
                && same_file(&second, &binary);
        if is_argv0_echo {
            args[3..].to_vec()
        } else {
            args[2..].to_vec()
        }
    } else {
        Vec::new()
    };

    match dispatch(&binary, &rest) {
        Ok(()) => unreachable!("execv does not return on success"),
        Err(message) => {
            // Everything except the bare refusal is worth a hint, because a
            // binfmt interpreter that just says "no" leaves the user with a
            // file manager that did nothing.
            fail(
                &format!("tahoe-dispatch: cannot run {}", binary.display()),
                &message,
            );
            EXIT_REFUSED
        }
    }
}

/// Identify, decide, and replace this process.
///
/// Returns `Err` with an explanation when the file should not run.
fn dispatch(binary: &Path, rest: &[OsString]) -> Result<(), String> {
    let command = resolve(binary, rest)?;
    let program = PathBuf::from(command.program());

    let mut process = Command::new(&program);
    process.args(&command.argv[1..]);
    if let Some(cwd) = &command.cwd {
        process.current_dir(cwd);
    }
    for (key, value) in &command.env {
        process.env(key, value);
    }

    // The point of the whole program: replace this process image rather than
    // forking. After this call there is no `tahoe-dispatch` in the tree.
    use std::os::unix::process::CommandExt;
    let error = process.exec();
    Err(format!("could not start {}: {error}", program.display()))
}

/// Identify and decide, returning the command that should replace us.
///
/// Separate from [`dispatch`] because a test cannot observe anything after
/// `execv`, and the decision is the part worth testing.
fn resolve(binary: &Path, rest: &[OsString]) -> Result<tahoe_uel::supervisor::CommandPlan, String> {
    let report = tahoe_format::probe_file(binary).map_err(|e| format!("cannot read it: {e}"))?;

    // The rule that got us here matched a byte pattern. Confirm that the
    // real parser agrees before handing anything to a compatibility layer.
    let expected = matches!(
        report.format,
        tahoe_core::ObjectFormat::Pe
            | tahoe_core::ObjectFormat::MachO
            | tahoe_core::ObjectFormat::FatMachO
    );
    if !expected {
        return Err(format!(
            "it is a {}, not a Windows or macOS program. The binfmt rule that \
             matched it is too broad — see docs/kernel-integration.md",
            report.format
        ));
    }

    let host = HostFacts::probe_current();
    // Speculative launches (an emulator, an unsandboxed AppImage) are an
    // explicit choice elsewhere, not something typing a filename should do.
    let policy = Policy {
        allow_cross_os: true,
        allow_speculative: false,
    };
    let layers = LayerPaths::from_path();
    let plan = decide(&report, &host, &policy, binary);

    if let Strategy::Rejected { reason } = &plan.strategy {
        return Err(format!("{reason:?}. {}", plan.summary));
    }

    let mut command = plan_command(&plan, binary, &layers).map_err(|e| e.to_string())?;
    command.argv.extend(rest.iter().cloned());
    Ok(command)
}

/// Whether two paths refer to the same file.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn fail(headline: &str, detail: &str) {
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "{headline}");
    let _ = writeln!(stderr, "  {detail}");
}

// `src/bin/<name>.rs` is a crate root, so a submodule needs an explicit
// path; the directory form that works for library modules does not apply.
#[cfg(test)]
#[path = "tahoe-dispatch/tests.rs"]
mod tests;
