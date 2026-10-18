# LICENSING.md — Project Tahoe

> **Status:** decision record. This file is normative for all first-party code.
> Last reviewed: 2026-10-13.

---

## 1. First-party license

**All code authored inside this repository is licensed `MIT OR Apache-2.0`**,
unless a file's header says otherwise.

This is the default dual license of the Rust ecosystem, it matches
[Smithay](https://github.com/Smithay/smithay) (the compositor library this
project is built on, `MIT`), and it keeps Tahoe's own components freely
embeddable by anyone.

Each Rust crate declares it in `Cargo.toml`:

```toml
license = "MIT OR Apache-2.0"
```

and carries an `SPDX-License-Identifier` header in every source file:

```rust
// SPDX-License-Identifier: MIT OR Apache-2.0
```

### Package-source license (Arch requirement)

Arch Linux requires that **package build sources** (PKGBUILDs, patches, install
scripts) be licensed `0BSD` and annotated with REUSE. See
<https://archlinux.org/news/providing-a-license-for-package-sources/> and
<https://wiki.archlinux.org/title/developer-manual:package-guidelines/rust>.
This repository therefore contains a `REUSE.toml` and `LICENSES/` directory for
the `packages/` and `iso/` trees. Runtime code stays `MIT OR Apache-2.0`.

---

## 2. Why not GPL-3.0?

This is the single most important licensing decision in the project, so the
reasoning is recorded here rather than in a commit message.

### 2.1 The dependency ledger (verified 2026-10-13)

| Dependency | License | How Tahoe uses it |
|---|---|---|
| Smithay (compositor) | `MIT` | linked into `tahoe-compositor` |
| smithay-client-toolkit (GUI) | `MIT` | linked into `tahoe-shell` |
| wayland-rs / wayland-server | `MIT` | linked into the compositor |
| FEX-Emu (x86→ARM64 emulator) | `MIT` | separate process, via UEL |
| box64 / box86 | `MIT` | separate process, via UEL |
| **Wine** | **`LGPL-2.1-or-later`** | separate process, via UEL |
| **Darling** | **`GPL-3.0-or-later`** | separate process, via UEL |
| Flatpak / xdg-desktop-portal | `LGPL-2.1-or-later` | separate process / D-Bus |
| archiso, mkinitcpio, systemd | `GPL-2.0` / `GPL-2.0-or-later` / `LGPL-2.1-or-later` | build + boot tooling |

### 2.2 The GPL-3.0 problem

Darling — the only realistic route to running macOS Mach-O binaries and `.app`
bundles on Linux — is `GPL-3.0-or-later`.

If Darling were **linked into** a Tahoe binary, the FSF's position (and ours) is
that the resulting combination is a single covered work:

> "Linking [your program] statically or dynamically with other modules is making
> a combined work based on [it]. Thus, the terms and conditions of the GNU
> General Public License cover the whole combination."
> — <https://www.gnu.org/licenses/gpl-faq.html#GPLIncompatibleLibs>

That would force every Tahoe component to be relicensed `GPL-3.0-or-later`.
We do not want that, and we do not need it.

### 2.3 The solution: process isolation, not relicensing

Wine and Darling are both **launchers**, not libraries. Their canonical command
lines are `wine <program>` and `darling <program>`; each owns a process, its own
prefix, and its own runtime. The Universal Execution Layer therefore *spawns*
them via `fork`/`exec` and talks to them over pipes / D-Bus.

This is exactly the boundary the GPL FAQ calls "mere aggregation":

> "By contrast, pipes, sockets and command-line arguments are communication
> mechanisms normally used between two separate programs."
> — <https://www.gnu.org/licenses/gpl-faq.html#MereAggregation>

Process isolation is also the *right* engineering choice regardless of law:

- a crashing compatibility layer cannot take down the compositor;
- each layer gets its own prefix, environment and cgroup;
- the compositor can supervise, restart, and resource-limit them;
- the layers can be updated independently of Tahoe.

### 2.4 Wine and LGPL-2.1

Even if Wine were linked directly, `LGPL-2.1-or-later` §6 explicitly permits
combining a "work that uses the Library" with the Library and distributing the
result "under terms of your choice", provided a suitable *shared-library*
mechanism is used and notices are preserved. So Wine is not a blocker either
way. We still prefer the process boundary.

### 2.5 If this ever changes

If a future requirement forces in-process linking of GPL-3.0 code, the fallback
is to relicense the **combined** binaries as `GPL-3.0-or-later` (the `-or-later`
on Darling gives version freedom), following the precedent of
[cosmic-comp](https://github.com/pop-os/cosmic-comp), which is `GPL-3.0-only`
and depends on `MIT` Smithay — i.e. GPL code depending on a permissive library.

That is the *reverse* direction of our problem and is unambiguously permitted.

---

## 3. Boundary rule (enforced, not aspirational)

> **No GPL-licensed code may ever be linked into a first-party Tahoe crate.**

Allowed: `dlopen` of an optional, separately-distributed, GPL-licensed plugin
that the user explicitly installs. Not allowed: `-l`, `#[link]`, or a Rust
`extern crate` dependency on a GPL crate.

This is checked in CI (see `docs/testing.md`, "License gate").

---

## 4. Third-party licenses we must respect

| Component | Obligation we accept |
|---|---|
| Wine (`LGPL-2.1-or-later`) | Dynamic linkage + notices + `wine` must remain user-replaceable |
| Darling (`GPL-3.0-or-later`) | Ship its license text; do not modify it without releasing sources |
| Flatpak (`LGPL-2.1-or-later`) | Dynamic linkage + notices |
| Smithay (`MIT`) | Preserve the copyright notice |
| **MacShell** (`MIT`, vendored live desktop) | Ship the MIT text (`LICENSES/MIT.txt`), keep the pinned provenance in `iso/macshell-vendor.md`, vendor the tree unmodified |
| Hyprland (`BSD-3-Clause`) / Quickshell (`LGPL-3.0-only`) | Session runtime binaries only — pulled by the archiso profile, never linked into first-party code |
| archiso (`GPL-3.0-or-later`) | Our derived profiles are `GPL-3.0-or-later` compatible |
| mkinitcpio (`GPL-2.0-only`) | Unmodified use |
| Linux kernel (`GPL-2.0`) | We do not modify it |

---

## 5. Non-goals

- We will **not** vendor, patch, or relicense Wine or Darling.
- We will **not** write our own PE or Mach-O loader. See
  `docs/adr/0005-no-custom-loader.md`.
- We will **not** claim that "format detection" implies "application
  compatibility". `tahoe-uel` deliberately reports a confidence class, not a
  guarantee. See `ARCHITECTURE.md` § "Honesty guarantees".
