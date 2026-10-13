# ARCHITECTURE.md — Project Tahoe

> This document describes the architecture **as designed**, and separates it
> explicitly from what is **implemented today**. Every claim about external
> projects was checked against its upstream source on 2026-10-13; the sources
> are listed in `docs/research/`.

---

## 0. What Tahoe is (and is not)

Project Tahoe is an **Arch Linux-based distribution** with a macOS-Tahoe-styled
Wayland desktop, built around a single conceptual idea:

> **A session where the user does not have to care which OS a program was
> written for — while the system is honest about how well that actually works.**

Concretely, Tahoe is:

- an Arch-derived base, built with `archiso`, packaged as pacman packages;
- a Rust Wayland compositor on [Smithay](https://github.com/Smithay/smithay);
- a shell (Dock, Launcher, Finder-like file manager, Control Center);
- a **Universal Execution Layer (UEL)** that identifies a binary and picks a
  *strategy* for running it, then supervises that strategy.

Tahoe is **not**:

- a new kernel, a new init system, or a new bootloader;
- a PE or Mach-O loader written by us;
- a claim of binary compatibility with Windows or macOS;
- a virtual machine or a CPU emulator.

---

## 1. Layer map

```
┌──────────────────────────────────────────────────────────────────────┐
│  L5  USER-visible desktop                                            │
│      Tahoe Shell — Dock, Launcher, Finder-like FM, Control Center,    │
│      Settings. Client-side toolkit: smithay-client-toolkit (MIT).    │
├──────────────────────────────────────────────────────────────────────┤
│  L4  SESSION & POLICY                                               │
│      tahoe-uel  — format identification, dependency probing,          │
│      strategy selection, supervision, telemetry of *success rates*   │
│      tahoe-service — service management, package/update state        │
├──────────────────────────────────────────────────────────────────────┤
│  L3  COMPOSITOR (Rust, Smithay, MIT)                                 │
│      tahoe-compositor — xdg-shell, layer-shell, seat/input, output    │
│      management, damage tracking, Xwayland, render backends          │
├──────────────────────────────────────────────────────────────────────┤
│  L2  BASE SYSTEM (Arch, unmodified)                                  │
│      systemd 262 · pacman 7 · mkinitcpio 42 · Linux kernel          │
│      App formats: Flatpak 1.18 (sandboxed), AppImage type 2 (not)     │
├──────────────────────────────────────────────────────────────────────┤
│  L1  COMPATIBILITY LAYERS — SEPARATE PROCESSES, never linked in       │
│      ┌───────────────┬──────────────────┬─────────────────────────┐  │
│      │ Linux ELF     │ Windows PE       │ macOS Mach-O / .app     │  │
│      │ kernel binfmt │ Wine 11 (LGPL)   │ Darling (GPL-3.0)       │  │
│      │ native        │ FEX-Emu, box64   │                         │  │
│      │               │ (MIT) for x86→arm│                         │  │
│      └───────────────┴──────────────────┴─────────────────────────┘  │
├──────────────────────────────────────────────────────────────────────┤
│  L0  KERNEL — Linux, upstream, unmodified. binfmt_misc (byte-magic    │
│      dispatch only, no semantic parsing).                            │
└──────────────────────────────────────────────────────────────────────┘
```

**The critical structural rule:** layers L1..L5 are separated by *process*
boundaries, and the compatibility layers at L1 are **never linked into** a
first-party binary. See `LICENSING.md` §3.

---

## 2. Components and their responsibilities

### 2.1 `tahoe-core` — shared domain types

Pure data types with no I/O and no dependencies on other crates:
`Arch`, `ObjectFormat`, `OsAbi`, `Endianness`, `AbiClass`. Deliberately tiny so
that `tahoe-format`, `tahoe-uel` and `tahoe-compositor` agree on one vocabulary.

### 2.2 `tahoe-format` — binary identification

**Responsibility:** read a file's leading bytes and answer *"what is this?"*
with as much structural detail as can be derived **without executing anything**.

It is a *parser*, not a policy engine. It reports facts:

| Format | What is actually parsed |
|---|---|
| ELF | `EI_CLASS`, `EI_DATA`, `EI_OSABI`, `e_type`, `e_machine`, `e_entry`, interpreter presence via `PT_INTERP` scan |
| PE | `MZ` magic, `e_lfanew`, `PE\0\0` signature, COFF machine, subsystem, `Characteristics` |
| Mach-O | `MH_MAGIC`/`MH_CIGAM`, `MH_MAGIC_64`/`MH_CIGAM_64`, `FAT_MAGIC`/`FAT_CIGAM`, `cputype` |
| AppImage | ELF header **plus** the `0x414902` (`AI\x02`) magic at offset 8 |
| DMG (UDIF) | header block signatures (`koly` trailer, `UBYTE`… see below) |
| Script | `#!` / `#!` variants, and shell/interpreter path |
| `.class` | `0xCAFEBABE` — reported as **ambiguous**, see below |

**Ambiguity is a first-class result.** `0xCAFEBABE` is both a Java class file
and a Mach-O *fat* (universal) header. `tahoe-format` returns
`ObjectFormat::Ambiguous { .. }` listing the candidates rather than guessing.
A wrong guess here means the UEL hands a Java class to Darling.

**Hard limits, documented rather than hidden:**
- a Mach-O fat header's `nfat_arch` count is at bytes 4–8, which *is* reachable,
  but `binfmt_misc` cannot express "fat *and* not `.class`", which is why
  identification lives in userspace (see §5).
- DMG detection is heuristic. A DMG's `koly` trailer is at the *end* of the
  file; header-block magic is not standardised across all UDIF variants. Where
  we cannot be certain we report `Low` confidence. We never execute a DMG.

### 2.3 `tahoe-uel` — Universal Execution Layer

**Responsibility:** turn an identification into an *execution plan*, and be
honest about how likely it is to work.

```
ProbeReport ──┐
              ├──► Policy + HostFacts ──► ExecutionPlan
Environment ──┘
```

An `ExecutionPlan` is:

```rust
pub enum Strategy {
    /// ELF matching the host arch → exec() directly.
    Native,
    /// Windows PE → Wine, in its own prefix.
    Wine { prefix: PathBuf, arch_mismatch: bool },
    /// Mach-O/.app → Darling, in its own DPREFIX.
    Darling { prefix: PathBuf },
    /// Needs an emulator underneath (e.g. x86-64 PE on an aarch64 host).
    Emulator { backend: EmulatorBackend, target_arch: Arch },
    /// A script we can run natively via its interpreter.
    Script { interpreter: PathBuf },
    /// Refuse, with a reason a human can act on.
    Rejected { reason: RejectionReason },
}
```

**Honesty guarantees.** `Strategy` never encodes "this will work". Alongside it,
`tahoe-uel` returns a `Confidence`:

```rust
pub enum Confidence {
    /// Same OS family, same arch, all declared interpreter deps present.
    Verified,
    /// Same OS family, same arch, some deps unverified.
    Plausible,
    /// Cross-OS translation layer: known to work for a *subset* of software.
    Layer { layer: &'static str, caveat: &'static str },
    /// We can attempt it, but nobody has measured this.
    Speculative,
    /// Do not attempt. RejectionReason explains.
    Refused,
}
```

This is the antidote to the usual failure mode of compatibility projects:
treating "we found `MZ` at offset 0" as "Windows apps work".

### 2.4 `tahoe-compositor` — the Wayland compositor

**Responsibility:** own the display server. Input, output, surfaces, focus,
rendering, and the Wayland protocol surface.

Built on Smithay 0.7 (`MIT`). Smithay is a *library*, not a framework — there
is no `smithay::run()`. We own `main()`, the event loop (`calloop`), the
`Display`/`DisplayHandle`, and the handler impls. The minimum trait set a
Smithay compositor must implement today is:

```
CompositorHandler · ShmHandler · BufferHandler · SeatHandler · XdgShellHandler
SelectionHandler · DataDeviceHandler · WaylandDndGrabHandler
PointerConstraintsHandler
```

registered with `smithay::delegate_dispatch2!(App)`, plus a
`wayland_server::backend::ClientData` per client.

**Backends.** Two, selected at compile time via Smithay's cargo features:

| Feature set | Use |
|---|---|
| `backend_winit` | nested compositor in a window — for development on an existing session |
| `backend_drm` + `backend_gbm` + `backend_egl` + `backend_libinput` + `backend_session_libseat` + `backend_udev` | real TTY, the shipping configuration |

Both were verified to **compile** on the reference machine
(`rustc 1.99.0`, see `docs/research/2026-10-13-smithay-build-verification.md`).

**Deliberate omissions:** no custom protocol extensions until xdg-shell +
layer-shell + the wlr protocols we already need are solid. A compositor that
implements five desktop-specific protocols badly is worse than one that
implements the standard ones well.

### 2.5 `tahoe-shell` — the desktop

**Responsibility:** everything the user sees that is not a window.

| Piece | Role |
|---|---|
| Dock | running/launched app list, window→app mapping |
| Launcher | `.desktop` file index, fuzzy query |
| Finder-like FM | a *browser*, not a clone — uses the portal file chooser |
| Control Center | network, audio, display, Bluetooth toggles |
| Settings | system preferences panes |

`tahoe-shell` owns a **model** (window list, workspace list, launcher index,
dock state) that is a plain Rust state machine with no rendering dependency,
and a **view** that renders it. The model is unit-testable without a GPU; this
split is why `tahoe-shell` exists as a crate rather than living inside the
compositor.

### 2.6 `tahoe-diagnostics` — health and honesty

**Responsibility:** read-only system probes. Is `binfmt_misc` mounted and
enabled? Which entries are registered, in what order? Are the Wine/Darling
binaries present and what version? Is the kernel new enough for NTSync?

This is the crate that lets a user (or a bug reporter) answer "why doesn't it
work" with facts instead of folklore.

---

## 3. Cross-cutting subsystems

### 3.1 Package management and updates

Tahoe inherits Arch's `pacman` and repository layout unmodified.

**Decision: no custom package manager.** A distro that forks the package
manager inherits every security fix forever, and we have no reason. Tahoe adds:

- `tahoe-repo-*` repositories, pacman-signed, alongside `core`/`extra`;
- an `archiso` profile (`iso/`) that produces a bootable installer;
- optional atomic-style updates via `systemd-sysupdate` for the *base image*
  only, if and when the base becomes an immutable image. Documented as a
  future option, not a day-one feature.

**Known caveat:** Arch's reproducible-builds effort
(<https://reproducible.archlinux.org/>) currently reports ~79% of recorded
builds bit-for-bit reproducible. Tahoe should not claim stronger guarantees
than its upstream.

### 3.2 Updates and security posture

- Unmodified `pacman` + `archlinux-keyring` signature chain.
- Flatpak apps are sandboxed (bubblewrap + seccomp + D-Bus proxy);
  **AppImage apps are not sandboxed at all** — this is a property of the
  format, not of Tahoe. AppImages are therefore treated as *untrusted
  executables* and launched with the UEL's `Rejected`/`Speculative` confidence
  plus a warning, never silently.
- The compatibility layers run as the unprivileged user, in their own prefix,
  with the compositor as supervisor. See `SECURITY.md`.

### 3.3 Diagnostics and logging

`tahoe-diagnostics` + `journalctl`. Every UEL decision is logged with the
`ProbeReport`, the chosen `Strategy`, and the `Confidence`, so that "it didn't
work" is always accompanied by "we chose X because Y".

---

## 4. Execution flow (the interesting part)

```
user double-clicks /Volumes/App.dmg
   │
   ▼
[1] tahoe-uel receives a path, NOT a binary
   │    DMG is a disk image. It is never exec()'d.
   ▼
[2] Mount / extract the image (read-only, FUSE or loop mount)
   │    → a mountpoint containing Foo.app/
   ▼
[3] Walk to Foo.app/Contents/MacOS/Foo   ← the actual Mach-O
   ▼
[4] tahoe-format probes it
   │    → Mach-O { cputype: x86_64, ... }
   ▼
[5] tahoe-uel builds a plan
   │    → Strategy::Darling { prefix }
   │    → Confidence::Layer { caveat: "Darling runs Mach-O via darlingserver; GUI AppKit support is minimal" }
   ▼
[6] Supervisor spawns:  darling --scratch-dir <prefix> /path/to/Foo
   │    pipes stdout/stderr, sets env, applies cgroup limits
   ▼
[7] Result recorded: success / failure, used for the dashboard in [8]
```

The same pipeline, with a different terminal step, serves:

```
PE  →  wine <path>          (Wine 11, new WoW64, no multilib needed)
ARM64 host + x86-64 PE  →  fex-emu / box64 wrapping wine  (Speculative)
ELF, host arch      →  exec() directly            (Verified)
.py / .sh / Shebang →  exec the interpreter       (Verified if interpreter present)
```

**What we deliberately do not do:** attempt to detect every PE/Mach-O variant
and silently pretend it will run.

---

## 5. Why identification lives in userspace, not in `binfmt_misc`

`binfmt_misc` is tempting: register `:DOSWin:M::MZ::/usr/bin/wine:` and Windows
programs "just run". Arch already ships exactly that in
`/usr/lib/binfmt.d/wine.conf`, and Tahoe will ship the same entries (see
`binfmt.d/`).

But `binfmt_misc` is a **byte-offset magic matcher with an AND-mask**. It
cannot:

- parse an ELF header, so it cannot dispatch on `e_type` or `e_machine` as
  *fields* — only on a hard-coded byte pattern at a fixed offset;
- express `PE\0\0`, whose offset comes from `e_lfanew` and is typically past
  the 128-byte registration window;
- express "fat Mach-O **and** not a Java `.class`";
- express "x86-64 PE **but** the host is aarch64, use FEX instead of Wine";
- report a confidence level or a caveat to anyone.

Therefore: `binfmt_misc` provides the *convenience* (`./program.exe` works),
and `tahoe-uel` provides the *decisioning*. The two coexist: binfmt handles the
trivial, already-handled cases; anything the UEL recognises as ambiguous,
cross-architecture, or cross-OS goes through the UEL's explicit path, which
sets the prefix, the environment, the emulator wrapper, and the confidence.

This is also why the UEL never trusts `binfmt_misc` ordering — the kernel docs
warn that "later added entries are matched first", making dispatch order
load-bearing and fragile across package installs.

---

## 6. Honesty guarantees (normative)

1. **Format identification ≠ OS compatibility.** `tahoe-format` may say
   `Mach-O` and `tahoe-uel` may say `Darling`. Neither says "macOS apps work".
   Darling's own maintainers state that GUI AppKit support is minimal; our
   `Confidence::Layer` carries that caveat.
2. **No fake success.** Test results in `docs/research/` are what actually
   happened. Where something is unverified it is labelled `UNVERIFIED`.
3. **Wine is not a Windows machine.** The new WoW64 mode in Wine 11 removed
   Arch's multilib requirement, but it has a documented regression: reduced
   performance for 32-bit applications that use OpenGL directly
   (bugs.winehq.org #55981).
4. **Darling runs userspace only.** Its kernel module was *removed* in 2022 in
   favour of `darlingserver`; there is nothing DKMS-related to maintain.
5. **DMG is data.** Always. Mounted, extracted, never executed.
6. **AppImage is not a sandbox.** Documented as such wherever it is mentioned.

---

## 7. Repository layout

```
project-tahoe/
├── Cargo.toml                  # workspace (`rust-version = "1.87"` MSRV)
├── README.md  ARCHITECTURE.md  ROADMAP.md  CONTRIBUTING.md
├── SECURITY.md  LICENSING.md
├── REUSE.toml  LICENSES/
├── crates/
│   ├── tahoe-core/             # domain types, no I/O
│   ├── tahoe-format/           # ELF / PE / Mach-O / AppImage / DMG / script parser
│   ├── tahoe-uel/              # strategy selection + confidence
│   ├── tahoe-compositor/       # Smithay compositor
│   ├── tahoe-shell/            # shell model (dock, launcher index, workspaces)
│   └── tahoe-diagnostics/      # read-only system probes
├── docs/
│   ├── research/               # dated, sourced findings
│   ├── adr/                    # architecture decision records
│   └── testing.md
├── iso/                        # archiso profile (GPL-3.0-or-later)
├── packages/                   # PKGBUILDs (0BSD sources)
├── binfmt.d/                   # shipped binfmt_misc entries
├── fixtures/                   # synthetic test binaries + generator
└── tests/                      # cross-crate integration tests
```

---

## 8. Architectural Decision Records

| ADR | Title |
|---|---|
| [0001](adr/0001-rust-and-smithay.md) | Rust + Smithay for the compositor |
| [0002](adr/0002-separate-compat-processes.md) | Compatibility layers as separate processes |
| [0003](adr/0003-no-custom-package-manager.md) | Inherit pacman, do not fork it |
| [0004](adr/0004-dmg-is-never-executed.md) | DMG is a disk image, handled as data |
| [0005](adr/0005-no-custom-loader.md) | No custom PE/Mach-O loader |
| [0006](adr/0006-uel-confidence-model.md) | The UEL reports confidence, not guarantees |
| [0007](adr/0007-binfmt-misc-vs-userspace-dispatch.md) | binfmt_misc for convenience, userspace for decisions |
