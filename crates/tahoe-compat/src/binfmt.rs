// SPDX-License-Identifier: MIT OR Apache-2.0
//! Rootless, isolated `binfmt_misc` dispatch.
//!
//! # Why this exists
//!
//! `binfmt_misc` is the only way Linux lets userspace influence what
//! `execve` does with a file that is not ELF — there will never be a
//! syscall to register a `linux_binfmt`, and the 7.3 BPF-backed handlers
//! require `CAP_BPF` in the initial user namespace. So it is the mechanism,
//! or there is none.
//!
//! Registering entries system-wide needs root and mutates a shared
//! resource that every other package also writes to. Since Linux 6.7
//! (commit `21ca59b365c0`, "binfmt_misc: enable sandboxed mounts") the
//! filesystem is `FS_USERNS_MOUNT`, so it can be mounted inside a user
//! namespace. That gives a strictly better shape for a distribution:
//!
//! - **No root.** `unshare -Ur -m` grants `CAP_SYS_ADMIN` *in the new
//!   namespace*, which is all the mount needs.
//! - **No host mutation.** The superblock is keyed by user namespace, so
//!   entries registered here are invisible to the rest of the system and
//!   vanish when the namespace exits. Verified on Linux 6.18.55.
//! - **Per-application scope.** A launcher can hand a child exactly the
//!   dispatch rules that child needs, and nothing else.
//!
//! For the cases that genuinely must be system-wide (a user double-clicking
//! a `.exe` in their file manager), the same entries are shipped as static
//! `binfmt.d/` files under `binfmt.d/` and applied by `systemd-binfmt` —
//! see `docs/kernel-integration.md`.
//!
//! # What this cannot do
//!
//! `binfmt_misc` is a byte-offset magic matcher with an AND-mask. Even
//! 7.3's improvements stay inside that model on stable kernels. It cannot
//! express "ELF with `e_machine == AArch64`" as a *predicate*, cannot
//! chain two conditions, cannot look past the prefetch window, and cannot
//! report a confidence. That is why format *decisions* stay in
//! `tahoe-uel` and only format *dispatch* comes here (ADR-0007).

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use tahoe_core::ObjectFormat;

/// Bytes the kernel prefetches into `bprm->buf`.
///
/// `BINPRM_BUF_SIZE` in `include/uapi/linux/binfmts.h`. Note the kernel
/// documentation still claims magic must be within the first 128 bytes;
/// the code has checked against this value since v5.1, so trust the code.
pub const BINPRM_BUF_SIZE: usize = 256;

/// Longest register string the kernel accepts (`MAX_REGISTER_LENGTH`).
pub const MAX_REGISTER_LENGTH: usize = 1920;

/// Mount point of the dispatcher.
pub const MOUNT_POINT: &str = "/proc/sys/fs/binfmt_misc";

/// Flags accepted in the register string on a stable kernel.
///
/// `T`, `L` and `D` exist only in 7.3 and later, so they are deliberately
/// absent: emitting a flag the running kernel does not know makes the whole
/// registration fail, and a silently ignored flag is worse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags {
    /// `P` — preserve `argv[0]` by prepending an extra argument instead of
    /// overwriting it. Required for any interpreter that wants to see the
    /// original name, which every launcher does.
    pub preserve_argv0: bool,
    /// `O` — pass the binary through `AT_EXECFD`, letting the interpreter
    /// run a file it cannot read. **Not** a security feature; it widens
    /// what the interpreter can reach.
    pub open_binary: bool,
    /// `C` — derive credentials from the binary rather than the
    /// interpreter. Implies `O`. The kernel documentation warns this runs
    /// the interpreter with the binary's privileges, so it is off by
    /// default and must be opted into deliberately.
    pub credentials: bool,
    /// `F` — open the interpreter at registration time and keep it, so the
    /// rule keeps working across mount-namespace and chroot changes. Costs
    /// one pinned fd per entry.
    pub fix_binary: bool,
}

impl Flags {
    /// The flag string as it appears in the register line.
    #[must_use]
    pub fn as_str(&self) -> String {
        let mut s = String::new();
        if self.preserve_argv0 {
            s.push('P');
        }
        if self.open_binary || self.credentials {
            s.push('O');
        }
        if self.credentials {
            s.push('C');
        }
        if self.fix_binary {
            s.push('F');
        }
        s
    }
}

/// One `binfmt_misc` rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Name of the entry file under the mount point.
    pub name: String,
    /// Magic bytes to match at [`Rule::offset`].
    pub magic: Vec<u8>,
    /// Mask, byte-for-byte the same length as `magic`. Empty means "match
    /// every byte", which is what the kernel defaults to.
    pub mask: Vec<u8>,
    /// Offset of the magic within the file.
    pub offset: usize,
    /// Absolute path of the interpreter to run.
    pub interpreter: PathBuf,
    /// Flags to append.
    pub flags: Flags,
}

/// Why a rule could not be expressed or registered.
#[derive(Debug)]
#[non_exhaustive]
pub enum BinfmtError {
    /// The magic would fall outside the kernel's prefetch window.
    MagicOutOfWindow {
        /// Last byte the match would need.
        needs: usize,
        /// How many bytes the kernel actually provides.
        available: usize,
    },
    /// The mask is not the same length as the magic.
    MaskLength {
        /// Magic length.
        magic: usize,
        /// Mask length.
        mask: usize,
    },
    /// The interpreter path is not absolute, which the kernel requires for
    /// any entry that will be opened at exec time.
    InterpreterNotAbsolute(PathBuf),
    /// The entry name contains a slash or is empty.
    InvalidName(String),
    /// The register string exceeds [`MAX_REGISTER_LENGTH`].
    RegisterStringTooLong(usize),
    /// The kernel rejected the registration.
    Rejected(String),
    /// The mount point is not a mounted `binfmt_misc` filesystem.
    NotMounted(String),
    /// Spawning the namespace helper failed.
    Helper(std::io::Error),
}

impl std::fmt::Display for BinfmtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MagicOutOfWindow { needs, available } => write!(
                f,
                "magic ends at byte {needs} but the kernel only prefetches {available}"
            ),
            Self::MaskLength { magic, mask } => {
                write!(f, "mask is {mask} bytes but magic is {magic}")
            }
            Self::InterpreterNotAbsolute(p) => {
                write!(f, "interpreter {} must be an absolute path", p.display())
            }
            Self::InvalidName(n) => write!(f, "invalid entry name {n:?}"),
            Self::RegisterStringTooLong(n) => write!(
                f,
                "register string is {n} bytes, limit is {MAX_REGISTER_LENGTH}"
            ),
            Self::Rejected(m) => write!(f, "kernel rejected the rule: {m}"),
            Self::NotMounted(m) => write!(f, "{MOUNT_POINT} is not a binfmt_misc mount: {m}"),
            Self::Helper(e) => write!(f, "namespace helper failed: {e}"),
        }
    }
}

impl std::error::Error for BinfmtError {}

/// Escape bytes into the `\xNN` form the register file expects.
///
/// The kernel parses `\xNN` inside the magic and mask fields and stops at a
/// raw NUL, so *every* byte is escaped rather than only the awkward ones.
#[must_use]
pub fn escape(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 4);
    for b in bytes {
        let _ = write!(out, "\\x{b:02x}");
    }
    out
}

impl Rule {
    /// A rule matching `magic` at offset 0 with an all-bytes mask.
    #[must_use]
    pub fn new(name: &str, magic: &[u8], interpreter: impl Into<PathBuf>) -> Self {
        Self {
            name: name.to_owned(),
            magic: magic.to_vec(),
            mask: Vec::new(),
            offset: 0,
            interpreter: interpreter.into(),
            flags: Flags {
                preserve_argv0: true,
                ..Flags::default()
            },
        }
    }

    /// Set a mask, one byte per magic byte; `0x00` means "ignore this byte".
    #[must_use]
    pub fn with_mask(mut self, mask: &[u8]) -> Self {
        self.mask = mask.to_vec();
        self
    }

    /// Set the magic offset.
    #[must_use]
    pub fn at_offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Set the flags.
    #[must_use]
    pub fn with_flags(mut self, flags: Flags) -> Self {
        self.flags = flags;
        self
    }

    /// Validate everything the kernel will check, so a failure is a typed
    /// error instead of a string from the kernel.
    ///
    /// # Errors
    ///
    /// See [`BinfmtError`].
    pub fn validate(&self) -> Result<(), BinfmtError> {
        if self.name.is_empty() || self.name.contains('/') {
            return Err(BinfmtError::InvalidName(self.name.clone()));
        }
        if !self.interpreter.is_absolute() {
            return Err(BinfmtError::InterpreterNotAbsolute(
                self.interpreter.clone(),
            ));
        }
        let needs = self.offset + self.magic.len();
        if needs > BINPRM_BUF_SIZE {
            return Err(BinfmtError::MagicOutOfWindow {
                needs,
                available: BINPRM_BUF_SIZE,
            });
        }
        if !self.mask.is_empty() && self.mask.len() != self.magic.len() {
            return Err(BinfmtError::MaskLength {
                magic: self.magic.len(),
                mask: self.mask.len(),
            });
        }
        let line = self.to_register_string();
        if line.len() > MAX_REGISTER_LENGTH {
            return Err(BinfmtError::RegisterStringTooLong(line.len()));
        }
        Ok(())
    }

    /// Render the line written to the `register` file.
    ///
    /// Layout: `:name:M:offset:magic:mask:interpreter:flags`.
    #[must_use]
    pub fn to_register_string(&self) -> String {
        format!(
            ":{}:M:{}:{}:{}:{}:{}",
            self.name,
            self.offset,
            escape(&self.magic),
            escape(&self.mask),
            self.interpreter.display(),
            self.flags.as_str()
        )
    }
}

/// A rule plus the evidence that it is live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredRule {
    /// The rule as registered.
    pub rule: Rule,
    /// Interpreter the kernel reports back, which is how a mismatch between
    /// what we intended and what is installed becomes visible.
    pub reported_interpreter: String,
    /// Flags string the kernel reported.
    pub reported_flags: String,
}

/// The dispatcher as seen from a running system or namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatcher {
    /// Whether `binfmt_misc` is mounted at [`MOUNT_POINT`].
    pub mounted: bool,
    /// Global enable switch, when readable.
    pub enabled: Option<bool>,
    /// Entry names present.
    pub entries: Vec<String>,
}

impl Dispatcher {
    /// Inspect the current namespace's dispatcher. Never fails.
    #[must_use]
    pub fn inspect() -> Self {
        let dir = Path::new(MOUNT_POINT);
        let mounted = dir.join("status").is_file() && dir.join("register").is_file();
        let enabled = fs::read_to_string(dir.join("status"))
            .ok()
            .map(|c| c.trim().eq_ignore_ascii_case("enabled"));
        let mut entries = Vec::new();
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name != "register" && name != "status" {
                    entries.push(name);
                }
            }
            entries.sort();
        }
        Self {
            mounted,
            enabled,
            entries,
        }
    }

    /// Read one registered entry back.
    ///
    /// # Errors
    ///
    /// Returns [`BinfmtError::NotMounted`] when the entry does not exist.
    pub fn read_entry(name: &str) -> Result<RegisteredRule, BinfmtError> {
        let path = Path::new(MOUNT_POINT).join(name);
        let text = fs::read_to_string(&path)
            .map_err(|e| BinfmtError::NotMounted(format!("{}: {e}", path.display())))?;
        Ok(parse_entry(name, &text))
    }
}

/// Parse the kernel's rendering of an entry.
///
/// The format is `enabled`, `interpreter <path>`, `flags: <flags>`,
/// `offset <n>`, `magic <hex>`.
#[must_use]
pub fn parse_entry(name: &str, text: &str) -> RegisteredRule {
    let mut interpreter = String::new();
    let mut flags = String::new();
    let mut offset = 0usize;
    let mut magic_hex = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("interpreter ") {
            interpreter = rest.trim().to_owned();
        } else if let Some(rest) = line.strip_prefix("flags:") {
            flags = rest.trim().to_owned();
        } else if let Some(rest) = line.strip_prefix("offset ") {
            offset = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("magic ") {
            magic_hex = rest.trim().to_owned();
        }
    }
    RegisteredRule {
        rule: Rule {
            name: name.to_owned(),
            magic: unhex(&magic_hex),
            mask: Vec::new(),
            offset,
            interpreter: PathBuf::from(interpreter.clone()),
            flags: Flags::default(),
        },
        reported_interpreter: interpreter,
        reported_flags: flags,
    }
}

/// Decode the kernel's hex rendering of a magic.
///
/// The kernel prints the magic as one run of hex digits with no separator
/// (`magic cffaedfe`), so the string is chunked into byte pairs rather than
/// split on whitespace. Whitespace anywhere in the value is ignored, since
/// some kernels and some tooling insert it.
#[must_use]
pub fn unhex(text: &str) -> Vec<u8> {
    let digits: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    digits
        .as_bytes()
        .chunks(2)
        .filter(|c| c.len() == 2)
        .filter_map(|c| {
            let s = std::str::from_utf8(c).ok()?;
            u8::from_str_radix(s, 16).ok()
        })
        .collect()
}

/// Write a rule into the *current* mount namespace.
///
/// The caller must already be inside a namespace where [`MOUNT_POINT`] is a
/// `binfmt_misc` mount; use [`register_isolated`] to get one.
///
/// # Errors
///
/// Returns [`BinfmtError`] on validation failure, a missing mount, or a
/// kernel refusal.
pub fn register_in_current_namespace(rule: &Rule) -> Result<(), BinfmtError> {
    rule.validate()?;
    let register = Path::new(MOUNT_POINT).join("register");
    if !register.exists() {
        return Err(BinfmtError::NotMounted(format!(
            "{} does not exist",
            register.display()
        )));
    }
    // Write directly rather than through a shell: the register file is a
    // kernel interface, and there is no reason to involve one.
    fs::write(&register, rule.to_register_string())
        .map_err(|e| BinfmtError::Rejected(e.to_string()))
}

/// Register `rules` in a throwaway user and mount namespace, then run
/// `body` with the dispatcher active.
///
/// This is the whole trick: `unshare -Ur -m` grants `CAP_SYS_ADMIN` in the
/// new namespace, `binfmt_misc` declares `FS_USERNS_MOUNT`, and the
/// resulting superblock disappears with the namespace. The host's
/// dispatcher is never touched.
///
/// `body` runs inside the namespace; its stdout and stderr are inherited so
/// callers can capture them.
///
/// # Errors
///
/// Returns [`BinfmtError::Helper`] when `unshare` cannot be spawned, or
/// [`BinfmtError::Rejected`] when the kernel refuses a rule.
pub fn register_isolated(
    rules: &[Rule],
    body: &[String],
) -> Result<std::process::Output, BinfmtError> {
    for rule in rules {
        rule.validate()?;
    }

    // `unshare` takes a command to run, so the mount and the registrations
    // must happen inside one `sh -c`. Nothing is interpolated into the
    // script text: the register lines arrive as positional parameters.
    const SCRIPT: &str = "\
set -e
mount -t binfmt_misc binfmt_misc \"$MNT\" 2>/dev/null || true
while [ \"$1\" != -- ]; do printf '%s\\n' \"$1\" > \"$MNT/register\"; shift; done
shift
exec \"$@\"";

    let mut cmd = Command::new("unshare");
    cmd.arg("-Ur")
        .arg("-m")
        .arg("--")
        .arg("sh")
        .arg("-c")
        .arg(SCRIPT);
    cmd.arg("sh"); // $0, unused by the script
    for rule in rules {
        cmd.arg(rule.to_register_string());
    }
    cmd.arg("--");
    for arg in body {
        cmd.arg(arg);
    }
    cmd.env("MNT", MOUNT_POINT);

    cmd.output().map_err(BinfmtError::Helper)
}

/// Rules Tahoe ships, one per format that a stable kernel can dispatch.
///
/// Every entry here is *dispatch only*. Nothing decides whether running the
/// file is a good idea — `tahoe-uel` does that, with a confidence, before
/// any of this is reached.
///
/// `interpreter` is the directory holding `tahoe-dispatch`, so the caller
/// controls where it is installed.
///
/// **One helper for every format.** The alternative — `tahoe-dispatch-wine`
/// and `tahoe-dispatch-darling` — would bake the magic-to-layer mapping into
/// the rule, and the magic cannot express it: `ca fe ba be` is also a Java
/// class file, and `MZ` is also a DOS executable. The helper re-identifies
/// the file and decides, which is the only place that decision can be made
/// correctly.
#[must_use]
pub fn shipped_rules(interpreter_dir: &Path) -> Vec<Rule> {
    // PE: `MZ` at offset 0. This is what Arch's own wine package
    // registers.
    //
    // `PE\0\0` sits at `e_lfanew`, and it is worth being precise about
    // whether that is reachable: the kernel document claims magic must lie
    // in the first **128** bytes, but `BINPRM_BUF_SIZE` is **256** and the
    // code checks against that. So `e_lfanew` values of 0x80 (very common
    // for MSVC-linked executables) and 0xF8 both fit, while 0xFC and above
    // do not. Since it is not reachable for *every* PE, the DOS stub is
    // still the honest anchor — a `PE\0\0` rule would silently match only
    // a subset.
    let dispatch = interpreter_dir.join("tahoe-dispatch");
    let pe = Rule::new("tahoe-wine-pe", b"MZ", &dispatch);

    // Mach-O thin, 64-bit little-endian: `cf fa ed fe`.
    let macho64 = Rule::new("tahoe-macho64", &[0xcf, 0xfa, 0xed, 0xfe], &dispatch);

    // Mach-O thin, 32-bit little-endian: `ce fa ed fe`.
    let macho32 = Rule::new("tahoe-macho32", &[0xce, 0xfa, 0xed, 0xfe], &dispatch);

    // Universal binary: `ca fe ba be`. This magic is *also* the Java class
    // file signature, and `binfmt_misc` cannot express "and not a class
    // file" — `nfat_arch` sits at offset 4 and the collision is only
    // resolvable by reading it, which a mask cannot do. So the interpreter
    // must re-check the header and hand Java files back to the JVM. Order
    // does not help: this magic shares no bytes with the thin entries, so
    // at most one of them can ever match a given file.
    let macho_fat = Rule::new("tahoe-macho-fat", &[0xca, 0xfe, 0xba, 0xbe], &dispatch);

    vec![pe, macho64, macho32, macho_fat]
}

/// The file extension a format is conventionally given, for diagnostics
/// only. Never used for dispatch decisions.
#[must_use]
pub fn conventional_extension(format: &ObjectFormat) -> &'static str {
    match format {
        ObjectFormat::Elf | ObjectFormat::AppImage => "",
        ObjectFormat::Pe => "exe",
        ObjectFormat::MachO | ObjectFormat::FatMachO => "",
        ObjectFormat::Dmg => "dmg",
        ObjectFormat::Xar => "pkg",
        ObjectFormat::Ole2 => "msi",
        ObjectFormat::Plist => "plist",
        ObjectFormat::AppBundle => "app",
        ObjectFormat::Script => "sh",
        ObjectFormat::JavaClass => "class",
        _ => "",
    }
}

#[cfg(test)]
mod tests;
