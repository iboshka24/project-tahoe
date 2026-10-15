# What "native" means, and what has been proven

> The goal is that a user double-clicks a `.exe` or a `.dmg` and it works.
> This document states precisely how close that is, what was measured, and
> what is impossible **in principle** rather than merely unimplemented.
>
> Measured on 2026-10-15 on kernel 6.18.55, with **Wine 11.0 installed as a
> user-level Flatpak** — no root, no system package. The executable used is a
> **real Windows PE** compiled on this machine with `clang` and `lld-link`.

---

## 1. The result

```
$ ./installer.exe --silent /install
wine-argv: [/tmp/.../installer.exe] [--silent] [/install]
tahoe-pe-ok
```

That is a `.exe`, run through the **kernel's** binfmt handler, dispatched by
`tahoe-dispatch`, executed by real Wine, printing its own output, exit 0.
`tests/dispatch-e2e.sh` verifies all of it: **8 checks, 0 failures**.

The same file through the UEL:

```
$ tahoe-uel run hello.exe
... "format":"pe","arch":"x86-64","strategy":"Wine { prefix: "..." }",
    "confidence":"layer:wine",
    "outcome":{"exit_code":0,...,"duration_ms":4301}...
tahoe-pe-ok
```

## 2. "Native" has five meanings. Four are achievable.

| Meaning | Status |
|---|---|
| **1. No wrapper, no terminal, no manual command** | ✅ **Done.** The kernel runs the file. `./app.exe` works exactly like `./app`. |
| **2. One process, appearing like any other, with an icon and a window** | ✅ **Achievable, mostly done.** After `execv` the only process is Wine running the PE. |
| **3. Zero setup, works on a fresh install** | 🟡 **Next.** Wine must be present and a prefix created once. |
| **4. Double-click from a file manager** | 🟡 **Next.** Needs `.desktop`/MIME entries; the dispatcher already makes it possible. |
| **5. Performance and compatibility equal to Windows** | ❌ **Irreducible.** Wine is an implementation of the Windows API, not Windows. |

## 3. The process chain, which is the part that can be fixed

```text
naive:   shell → wrapper → supervisor thread → wine → program        (4 hops)
Tahoe:   kernel → tahoe-dispatch → wine → program                    (2 hops)
```

`tahoe-dispatch` calls `execv`, which **replaces its own process image**. It
does not fork, does not stay resident, and does not appear in the process
tree. After the call the chain is exactly what a `#!` script produces.

**Two hops is the floor.** The `execve` that starts Wine must come from
somewhere; Linux offers no third option between "the kernel knows the format"
(which it cannot, for PE) and "a userspace program does it".

Confirmed by inspection: `tahoe-dispatch` is absent from the process tree
once Wine is running, because it *is* Wine by then.

## 4. What is impossible in principle, and why

**There is no native PE execution on Linux, and there cannot be one.** Not
because the kernel is missing a feature, but because a PE file is not a
program description — it is a program that expects an operating system.

A PE binary's first action is to call into `ntdll.dll`. That call has to land
somewhere, and the target has to implement:

- **Syscalls**: `NtCreateFile`, `NtAllocateVirtualMemory`, hundreds more,
  with Windows semantics (including `PAGE_GUARD`, which Wine implements with
  `userfaultfd`).
- **The loader**: imports, relocations, TLS callbacks, SEH, delay-loading,
  the PEB and TEB.
- **The API**: `kernel32`, `user32`, `gdi32`, COM, the registry, and on top
  of those, thousands of behaviours applications depend on.

Kernel-space execution would not help: none of that belongs in a kernel, and
`docs/kernel-integration.md` records the same conclusion reached from the
other direction — the 7.3 BPF binfmt work is about *dispatch*, not about
implementing an OS.

Wine implements those three layers in userspace, and that is why it works at
all. Every other attempt (see `docs/universal-execution-research.md` §7) is
either a fork of Wine, a virtual machine, or a research project.

So: **"native .exe" is achieved to the extent that it can be** — one process,
no wrapper, kernel dispatch, a real window — *and the compatibility layer
itself is irreducible.*

## 5. `.dmg`: the same shape of answer

A `.dmg` is a **disk image**. There is nothing to "run" — the native
mechanism is the one the kernel already has: **mount it**. Tahoe's job is to
make that automatic and safe:

```text
detect (koly trailer, block table, compression)
  → mount read-only   ← the native mechanism, /dev/fuse or a loop device
  → find the .app
  → probe the Mach-O inside
  → plan for it
```

What is implemented today: detect, parse the block table, report the
compression methods, resolve a `.app` to its executable, parse the Mach-O.
What is missing is the middle step, and the reason is concrete: **no DMG
tooling is installed on this machine** (`7z`, `dmg2img`, `hfsfuse`,
`apfs-fuse` are all absent), and `libarchive` has no UDIF support at all.

Two honest options, in order of value:

1. **A UDIF decoder in Rust.** The block table gives per-chunk offsets and a
   compression type; `zlib`, `bzip2`, `raw` and `zero` are decodable without
   any external tool, and only LZFSE (Apple's) would need more. This removes
   the dependency entirely for the common case and is the next piece of work.
2. **Require one external tool** and document it. Cheaper, but a
   distribution should not depend on a package it cannot verify.

Either way, **the `.dmg` is never executed** — ADR-0004 — and that is not a
limitation, it is the correct behaviour.

## 6. What "native" the user actually gets

| | |
|---|---|
| Double-click a `.exe` in a shell | ✅ works today |
| `tahoe-uel run app.exe` with a confidence report | ✅ works today |
| The window appears like any other window | ✅ it is a normal Wayland client |
| Exit code, stdout, timeout, process-group cleanup | ✅ works today |
| A Dock entry with the program's icon | 🟡 `winemenubuilder` writes the `.desktop`; wiring it to the Dock is next |
| First launch is slow while the prefix is created | 🟡 fixable by provisioning at install time |
| Every Windows program works | ❌ not true, and no layer makes it true |

## 7. How this was verified, because the method matters

Wine is not installed as a system package; `sudo` on this machine requires a
password. It was installed as a **user-level Flatpak**
(`org.winehq.Wine//stable-25.08`, from Flathub), which needs no root and is
removable with `flatpak uninstall --user`.

The test executable is not a synthetic byte array: it is compiled on this
machine by `clang --target=x86_64-pc-windows-msvc` and linked by `lld-link`
against a hand-written `kernel32.dll` import library — both tools ship with
Arch, so no MinGW toolchain is needed. It calls `GetStdHandle`, `WriteFile`
and `ExitProcess`, which is enough to prove that a real PE runs through the
real stack and produces real output.

Reproduce:

```sh
# Wine, user-level, no root:
flatpak install --user flathub org.winehq.Wine//stable-25.08

# Then, with a `wine` on PATH that reaches it:
PATH=... WINEPREFIX=... tests/dispatch-e2e.sh
```

**Not verified:** any real-world Windows application. The fixture is a
program we wrote; Wine's compatibility with software we did not write is
Wine's to measure, and `tahoe-bench` deliberately does not claim otherwise.
