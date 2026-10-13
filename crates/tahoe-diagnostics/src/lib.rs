// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-diagnostics`: read-only probes that answer "why doesn't it work?"
//!
//! Everything here **reads**: `/proc` files, PATH lookups, `uname`. Nothing
//! registers, mounts, writes, or executes. Probes return data, never panic
//! on missing pieces — an absent layer is a finding, not a failure.

use std::path::{Path, PathBuf};

/// State of the kernel's `binfmt_misc` dispatcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinfmtState {
    /// Mounted at `/proc/sys/fs/binfmt_misc`.
    pub mounted: bool,
    /// Global `enabled` flag (None when unreadable).
    pub enabled: Option<bool>,
    /// Registered entry names (excluding `register`/`status`).
    pub entries: Vec<String>,
}

/// One compatibility layer binary found (or not) on PATH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerProbe {
    /// Binary name (`wine`, `darling`, …).
    pub name: &'static str,
    /// Full path when found.
    pub path: Option<PathBuf>,
}

/// Kernel features Tahoe cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelFacts {
    /// Raw release string (`6.18.55-1-cachyos-lts`).
    pub release: String,
    /// Parsed (major, minor, patch), when the release starts with one.
    pub version: Option<(u64, u64, u64)>,
    /// NTSync needs kernel ≥ 6.14 (module ships in-kernel, off by default).
    pub ntsync_capable: bool,
}

/// Parse `(major, minor, patch)` from a release like `6.18.55-1-cachyos-lts`.
#[must_use]
pub fn parse_kernel_version(release: &str) -> Option<(u64, u64, u64)> {
    let mut parts = release.split(['.', '-']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// Parse the content of `/proc/sys/fs/binfmt_misc/status`.
#[must_use]
pub fn parse_binfmt_status(content: &str) -> Option<bool> {
    match content.trim() {
        "enabled" => Some(true),
        "disabled" => Some(false),
        _ => None,
    }
}

/// Read `/proc/sys/fs/binfmt_misc` state. Never fails: absence is data.
#[must_use]
pub fn probe_binfmt() -> BinfmtState {
    let dir = Path::new("/proc/sys/fs/binfmt_misc");
    let mounted = dir.join("status").is_file() && dir.join("register").is_file();
    let enabled = std::fs::read_to_string(dir.join("status"))
        .ok()
        .and_then(|c| parse_binfmt_status(&c));
    let mut entries = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name != "register" && name != "status" && e.path().is_file() {
                entries.push(name);
            }
        }
        entries.sort();
    }
    BinfmtState {
        mounted,
        enabled,
        entries,
    }
}

/// Look up layer binaries on PATH.
#[must_use]
pub fn probe_layers() -> Vec<LayerProbe> {
    const LAYERS: &[&str] = &[
        "wine",
        "darling",
        "flatpak",
        "fex-emu",
        "box64",
        "qemu-x86_64",
    ];
    LAYERS
        .iter()
        .map(|name| LayerProbe {
            name,
            path: find_on_path(name),
        })
        .collect()
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// Read kernel facts via `uname -r` (no `libc` dependency: parse the file
/// the kernel already exposes through `/proc/sys/kernel/osrelease`).
#[must_use]
pub fn probe_kernel() -> KernelFacts {
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();
    let version = parse_kernel_version(&release);
    let ntsync_capable = version.is_some_and(|(maj, min, _)| (maj, min) >= (6, 14));
    KernelFacts {
        release,
        version,
        ntsync_capable,
    }
}

/// Full snapshot: binfmt + layers + kernel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsReport {
    /// binfmt_misc state.
    pub binfmt: BinfmtState,
    /// Layer binaries.
    pub layers: Vec<LayerProbe>,
    /// Kernel facts.
    pub kernel: KernelFacts,
}

/// Collect everything. Read-only.
#[must_use]
pub fn collect() -> DiagnosticsReport {
    DiagnosticsReport {
        binfmt: probe_binfmt(),
        layers: probe_layers(),
        kernel: probe_kernel(),
    }
}

#[cfg(test)]
mod tests;
