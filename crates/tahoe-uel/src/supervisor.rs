// SPDX-License-Identifier: MIT OR Apache-2.0
//! The supervisor: turning an [`ExecutionPlan`] into a running process.
//!
//! This is the half of the UEL that touches the system. It is deliberately
//! the smallest possible surface, because it is the part that can hurt you:
//!
//! - **No shell, ever.** Every command is an explicit `argv` vector passed
//!   straight to `execve`. A filename containing `;` or `$(…)` is a filename,
//!   not syntax. There is a test for that.
//! - **A whitelisted environment.** Children see a fixed set of variables,
//!   not whatever happened to be in the launcher's environment.
//! - **A real timeout that also kills the process group.** Killing only the
//!   direct child of a compatibility layer leaves orphans holding the
//!   display, so the child is put in its own process group and the whole
//!   group is signalled.
//! - **Bounded output capture.** A chatty or hostile program cannot make the
//!   launcher allocate without limit.
//! - **Refusals are refused.** A [`Strategy::Rejected`] plan never spawns
//!   anything.
//!
//! Stage 2 of the roadmap puts this behind the compositor. Today it is a
//! library with tests, and a CLI that can be pointed at a file.

use std::{
    ffi::{OsStr, OsString},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use tahoe_core::Arch;

use crate::{Confidence, EmulatorBackend, ExecutionPlan, RejectionReason, Strategy};

/// Where the compatibility layers live.
///
/// Injected rather than looked up, so tests exercise the real code path
/// without Wine installed and so a caller can pin an exact binary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayerPaths {
    /// `wine`, or a wrapper such as `wine-staging`.
    pub wine: Option<PathBuf>,
    /// `darling`.
    pub darling: Option<PathBuf>,
    /// `fex-emu` / `FEXInterpreter`.
    pub fex: Option<PathBuf>,
    /// `box64`.
    pub box64: Option<PathBuf>,
    /// `qemu-<arch>` user-mode emulator.
    pub qemu_user: Option<PathBuf>,
}

impl LayerPaths {
    /// Look the layers up on `PATH`. Never fails: absence is a finding.
    #[must_use]
    pub fn from_path() -> Self {
        let find = |names: &[&str]| -> Option<PathBuf> {
            let paths = std::env::var_os("PATH")?;
            for name in names {
                for dir in std::env::split_paths(&paths) {
                    let candidate = dir.join(name);
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
            None
        };
        Self {
            wine: find(&["wine"]),
            darling: find(&["darling"]),
            fex: find(&["FEXInterpreter", "fex-emu"]),
            box64: find(&["box64"]),
            qemu_user: find(&["qemu-x86_64", "qemu-aarch64", "qemu-riscv64"]),
        }
    }

    fn emulator(&self, backend: EmulatorBackend) -> Option<&Path> {
        match backend {
            EmulatorBackend::Fex => self.fex.as_deref(),
            EmulatorBackend::Box64 => self.box64.as_deref(),
            EmulatorBackend::QemuUser => self.qemu_user.as_deref(),
        }
    }
}

/// Environment variables a child is allowed to inherit.
///
/// Everything else is dropped. The list is short on purpose: a launcher that
/// forwards `LD_PRELOAD`, `PYTHONPATH` or `GIT_*` into an untrusted
/// compatibility layer is handing it the user's session.
pub const ENV_WHITELIST: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TERM",
    "TZ",
    "XDG_RUNTIME_DIR",
    "XDG_DATA_HOME",
    "XDG_CONFIG_HOME",
    "XDG_CACHE_HOME",
    "XDG_DATA_DIRS",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XAUTHORITY",
    "DBUS_SESSION_BUS_ADDRESS",
    "PULSE_SERVER",
];

/// A concrete, ready-to-execute command. Still just data — nothing has run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPlan {
    /// `argv[0]` and its arguments, in order.
    pub argv: Vec<OsString>,
    /// Variables to set on top of the whitelist.
    pub env: Vec<(OsString, OsString)>,
    /// Working directory, when the caller asked for one.
    pub cwd: Option<PathBuf>,
    /// The layer this command goes through, for logging.
    pub layer: Option<&'static str>,
}

impl CommandPlan {
    /// `argv[0]`.
    #[must_use]
    pub fn program(&self) -> &OsStr {
        &self.argv[0]
    }

    /// A single line suitable for a log or an error message.
    #[must_use]
    pub fn command_line(&self) -> String {
        self.argv
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Why a plan could not become a command.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlanError {
    /// The UEL refused this file. Carries the UEL's own reason.
    Refused(RejectionReason),
    /// The strategy is sound but the layer binary is not installed.
    LayerMissing {
        /// `wine`, `darling`, the emulator, …
        layer: &'static str,
    },
    /// Nothing to execute.
    EmptyProgram,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(r) => write!(f, "refused: {r:?}"),
            Self::LayerMissing { layer } => write!(f, "{layer} is not installed"),
            Self::EmptyProgram => write!(f, "empty program path"),
        }
    }
}

impl std::error::Error for PlanError {}

/// Build the command for a plan, or explain why there isn't one.
///
/// Pure: inspects the plan and the injected layer paths, touches nothing.
///
/// # Errors
///
/// Returns [`PlanError::Refused`] for a rejected strategy,
/// [`PlanError::LayerMissing`] when the required layer is absent, and
/// [`PlanError::EmptyProgram`] for an empty path.
pub fn plan_command(
    plan: &ExecutionPlan,
    program: &Path,
    layers: &LayerPaths,
) -> Result<CommandPlan, PlanError> {
    if program.as_os_str().is_empty() {
        return Err(PlanError::EmptyProgram);
    }
    let program = program.as_os_str().to_owned();
    let plain = |argv: Vec<OsString>, layer: Option<&'static str>| CommandPlan {
        argv,
        env: Vec::new(),
        cwd: None,
        layer,
    };
    let need = |name: &'static str, path: Option<&Path>| -> Result<OsString, PlanError> {
        path.map(|p| p.as_os_str().to_owned())
            .ok_or(PlanError::LayerMissing { layer: name })
    };

    match &plan.strategy {
        Strategy::Rejected { reason } => Err(PlanError::Refused(reason.clone())),
        Strategy::Native => Ok(plain(vec![program], None)),
        Strategy::Script {
            interpreter,
            interpreter_args,
        } => {
            let mut argv = Vec::with_capacity(2 + interpreter_args.len());
            argv.push(interpreter.as_os_str().to_owned());
            argv.extend(interpreter_args.iter().map(OsString::from));
            argv.push(program);
            Ok(plain(argv, None))
        }
        Strategy::Wine { prefix } => {
            let wine = need("wine", layers.wine.as_deref())?;
            let mut cmd = plain(vec![wine, program], Some("wine"));
            cmd.env
                .push(("WINEPREFIX".into(), prefix.as_os_str().to_owned()));
            // Wine must not try to fetch gecko/mono from the network while a
            // program is starting; the packager decides when to install them.
            cmd.env
                .push(("WINEDLLOVERRIDES".into(), "mscoree,mshtml=".into()));
            Ok(cmd)
        }
        Strategy::Darling { prefix } => {
            // UNVERIFIED against a real Darling install: documented usage is
            // `darling <command>`, and every reference here is upstream's
            // README rather than an observed run. The prefix is passed as an
            // environment variable, which is what Darling documents
            // (`DPREFIX`). Re-check when a build is available.
            let darling = need("darling", layers.darling.as_deref())?;
            let mut cmd = plain(vec![darling, program], Some("darling"));
            cmd.env
                .push(("DPREFIX".into(), prefix.as_os_str().to_owned()));
            Ok(cmd)
        }
        Strategy::Emulator { backend } => {
            let name = match backend {
                EmulatorBackend::Fex => "fex-emu",
                EmulatorBackend::Box64 => "box64",
                EmulatorBackend::QemuUser => "qemu-user",
            };
            let emulator = need(name, layers.emulator(*backend))?;
            Ok(plain(vec![emulator, program], Some(name)))
        }
    }
}

/// Limits and hygiene rules for a launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPolicy {
    /// Kill the process group after this long. `None` means no limit, which
    /// is only appropriate for a program the user is watching interactively.
    pub timeout: Option<Duration>,
    /// Maximum bytes captured from each of stdout and stderr.
    pub capture_limit: usize,
    /// Extra variables the caller vouches for, added to [`ENV_WHITELIST`].
    ///
    /// Applied **after** the [`CommandPlan`]'s own variables, so an explicit
    /// human override always beats an automatically derived default such as
    /// `WINEPREFIX`.
    pub extra_env: Vec<(OsString, OsString)>,
}

impl Default for LaunchPolicy {
    fn default() -> Self {
        Self {
            timeout: Some(Duration::from_secs(300)),
            capture_limit: 64 * 1024,
            extra_env: Vec::new(),
        }
    }
}

/// How a launch ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOutcome {
    /// Exit code, when the process exited normally.
    pub exit_code: Option<i32>,
    /// Signal that terminated the process, when it was signalled.
    pub signal: Option<i32>,
    /// Captured stdout, lossily decoded, truncated to the capture limit.
    pub stdout: String,
    /// Captured stderr, lossily decoded, truncated to the capture limit.
    pub stderr: String,
    /// Wall-clock duration.
    pub duration: Duration,
    /// Whether the supervisor had to kill it.
    pub timed_out: bool,
    /// Whether capture was cut short by the limit.
    pub stdout_truncated: bool,
    /// Whether capture was cut short by the limit.
    pub stderr_truncated: bool,
}

impl LaunchOutcome {
    /// True when the process exited with status 0 and was not killed.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        !self.timed_out && self.exit_code == Some(0)
    }

    /// One-line summary for a journal.
    #[must_use]
    pub fn summary(&self) -> String {
        let status = if self.timed_out {
            "timed out and was killed".to_owned()
        } else if let Some(code) = self.exit_code {
            format!("exited {code}")
        } else if let Some(sig) = self.signal {
            format!("killed by signal {sig}")
        } else {
            "ended for an unknown reason".to_owned()
        };
        format!("{status} after {:?}", self.duration)
    }
}

/// Why a launch could not even start.
#[derive(Debug)]
#[non_exhaustive]
pub enum LaunchError {
    /// The command could not be built. Nothing ran.
    Plan(PlanError),
    /// The program does not exist.
    NotFound(PathBuf),
    /// The program exists but is not executable.
    NotExecutable(PathBuf),
    /// `execve` or the pipe setup failed.
    Io(std::io::Error),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Plan(e) => write!(f, "{e}"),
            Self::NotFound(p) => write!(f, "{} does not exist", p.display()),
            Self::NotExecutable(p) => write!(f, "{} is not executable", p.display()),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LaunchError {}

/// Pre-flight checks that produce a better message than `execve` would.
fn check_runnable(path: &Path) -> Result<(), LaunchError> {
    let meta = std::fs::metadata(path).map_err(|_| LaunchError::NotFound(path.to_path_buf()))?;
    if meta.is_dir() {
        return Err(LaunchError::NotExecutable(path.to_path_buf()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return Err(LaunchError::NotExecutable(path.to_path_buf()));
        }
    }
    Ok(())
}

/// Run a command, capturing bounded output, honouring the timeout, and
/// killing the whole process group if it overruns.
///
/// # Errors
///
/// Returns [`LaunchError`] when the command cannot be built or started. A
/// child that exits non-zero is **not** an error: it is an outcome.
pub fn spawn(command: &CommandPlan, policy: &LaunchPolicy) -> Result<LaunchOutcome, LaunchError> {
    let program = PathBuf::from(command.program());
    check_runnable(&program)?;

    let mut cmd = Command::new(&program);
    cmd.args(&command.argv[1..]);
    if let Some(cwd) = &command.cwd {
        cmd.current_dir(cwd);
    }

    // Start from a bare environment: only the whitelist and what the caller
    // explicitly vouched for.
    cmd.env_clear();
    for key in ENV_WHITELIST {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    // Plan first, policy second: explicit overrides win.
    for (key, value) in command.env.iter().chain(policy.extra_env.iter()) {
        cmd.env(key, value);
    }

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    // Own process group, so a timeout can take the whole tree down instead
    // of orphaning whatever the compatibility layer spawned.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let started = Instant::now();
    let mut child = cmd.spawn().map_err(LaunchError::Io)?;
    let pgid = rustix::process::Pid::from_raw(child.id() as i32);

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let limit = policy.capture_limit;
    let stdout_reader = std::thread::spawn(move || read_capped(stdout, limit));
    let stderr_reader = std::thread::spawn(move || read_capped(stderr, limit));

    // Poll rather than block, so the deadline is honoured. A blocked
    // `wait()` cannot be interrupted, and there is no `wait_timeout` in std.
    let mut timed_out = false;
    let mut signal = None;
    let mut exit_code = None;
    let poll = Duration::from_millis(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                exit_code = status.code();
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    signal = status.signal();
                }
                break;
            }
            Ok(None) => {}
            Err(e) => return Err(LaunchError::Io(e)),
        }

        if let Some(limit) = policy.timeout
            && started.elapsed() >= limit
        {
            timed_out = true;
            if let Some(pgid) = pgid {
                // A negative pid targets the whole group, so whatever the
                // compatibility layer spawned dies with it.
                let _ = rustix::process::kill_process_group(pgid, rustix::process::Signal::Kill);
            }
            let _ = child.kill();
            if let Ok(status) = child.wait() {
                exit_code = status.code();
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    signal = status.signal();
                }
            }
            break;
        }
        std::thread::sleep(poll);
    }

    let (stdout, stdout_truncated) = stdout_reader.join().unwrap_or_default();
    let (stderr, stderr_truncated) = stderr_reader.join().unwrap_or_default();

    Ok(LaunchOutcome {
        exit_code,
        signal,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        duration: started.elapsed(),
        timed_out,
        stdout_truncated,
        stderr_truncated,
    })
}

/// Read a stream up to `limit` bytes, then keep draining without storing.
///
/// Draining matters: closing the pipe early would deliver `EPIPE` to a
/// program that did nothing wrong.
fn read_capped(stream: Option<impl Read>, limit: usize) -> (Vec<u8>, bool) {
    let Some(mut stream) = stream else {
        return (Vec::new(), false);
    };
    let mut kept = Vec::with_capacity(limit.min(8192));
    let mut truncated = false;
    let mut buf = [0u8; 8192];
    loop {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if kept.len() < limit {
                    let room = limit - kept.len();
                    let take = n.min(room);
                    kept.extend_from_slice(&buf[..take]);
                    if take < n {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
        }
    }
    (kept, truncated)
}

/// A structured record of one launch decision and its result.
///
/// This is what a support request should contain: what we identified, what
/// we chose, how confident we were, and what actually happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchRecord {
    /// The file the user asked to run.
    pub requested: PathBuf,
    /// Format and architecture, as identified.
    pub format: String,
    /// Architecture, as identified.
    pub arch: Arch,
    /// The strategy the UEL chose.
    pub strategy: String,
    /// The supervisor's belief in that strategy.
    pub confidence: Confidence,
    /// The command that was (or would have been) run.
    pub command: Option<String>,
    /// The outcome, when something ran.
    pub outcome: Option<LaunchOutcome>,
    /// The refusal, when nothing ran.
    pub error: Option<String>,
}

impl LaunchRecord {
    /// A one-line JSON-ish rendering. Hand-rolled to keep the crate free of
    /// a serialization dependency for a single log line.
    #[must_use]
    pub fn to_log_line(&self) -> String {
        let escape = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let outcome = self.outcome.as_ref().map_or_else(
            || "null".to_owned(),
            |o| {
                format!(
                    "{{\"exit_code\":{},\"timed_out\":{},\"duration_ms\":{},\"stdout_truncated\":{},\"stderr_truncated\":{}}}",
                    o.exit_code.map_or("null".to_owned(), |c| c.to_string()),
                    o.timed_out,
                    o.duration.as_millis(),
                    o.stdout_truncated,
                    o.stderr_truncated,
                )
            },
        );
        format!(
            "{{\"requested\":\"{}\",\"format\":\"{}\",\"arch\":\"{}\",\"strategy\":\"{}\",\"confidence\":\"{}\",\"command\":{},\"outcome\":{},\"error\":{}}}",
            escape(&self.requested.to_string_lossy()),
            escape(&self.format),
            self.arch,
            escape(&self.strategy),
            escape(&self.confidence.to_string()),
            self.command
                .as_ref()
                .map_or("null".to_owned(), |c| format!("\"{}\"", escape(c))),
            outcome,
            self.error
                .as_ref()
                .map_or("null".to_owned(), |e| format!("\"{}\"", escape(e))),
        )
    }
}

#[cfg(test)]
mod tests;
