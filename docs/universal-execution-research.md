# Universal Execution: architecture research

> Phase 1 deliverable. Every claim about an external project was checked
> against its upstream source or documentation on **2026-10-14**; sources are
> cited inline. A claim I could not verify is marked **UNVERIFIED**.
>
> Machine used for the empirical checks: CachyOS, kernel
> `6.18.55-1-cachyos-lts`, x86_64, Rust 1.99.0.

---

## 0. The question, stated precisely

"Run Windows and macOS apps natively" is not one problem. It is six, and
they have different physics:

| Layer | What it is | Solved by | Solved by a kernel patch? |
|---|---|---|---|
| **Format** | What bytes is this? | a parser, in userspace | no — and should not be |
| **ABI** | How does a call into the OS look? | a loader + thunking (Wine's `wow64`, Darling's `darlingserver`) | no |
| **System calls** | Which kernel operations does it need? | translation to Linux syscalls | partially, but userspace is fine |
| **API** | `kernel32`/`user32`/`Foundation`/`AppKit` | **reimplementation — years of work** | absolutely not |
| **Graphics/audio/input** | How does it reach a display? | a driver inside the layer (`winewayland.drv`) | no |
| **Frameworks** | Swift, Metal, AppKit, anti-cheat, kernel drivers | mostly *not solved at all* | no |

The single most important finding of this research is the boundary between
rows 1–3 and rows 4–6. **Rows 1–3 are engineering that can be finished.
Rows 4–6 are why Wine is not Windows and Darling is not macOS, and no
amount of kernel work moves them.**

That distinction drove every decision below.

---

## 1. What the kernel actually offers

### 1.1 The exec path

`search_binary_handler()` walks a list of `struct linux_binfmt` handlers.
A handler that returns `-ENOEXEC` lets the walk continue; anything else
stops it. Handlers may rewrite `bprm->interpreter`, and the rewrite chain is
capped at **5 levels** (`-ELOOP` beyond that).

- `BINPRM_BUF_SIZE` is **256**, in `include/uapi/linux/binfmts.h`. The kernel
  prefetches 256 bytes and zero-pads.
  *Note:* the documentation at
  <https://www.kernel.org/doc/html/latest/admin-guide/binfmt-misc.html>
  claims magic must lie in the first **128** bytes. The code checks
  `BINPRM_BUF_SIZE`. Trust the code; this is a live documentation bug.
- **There is no syscall, netlink socket or `/sys` file to register a
  `linux_binfmt` from userspace**, and the autoloading of `binfmt-*` modules
  was removed in v6.14 (`fa1bdca98d74`). `register_binfmt()` is
  `EXPORT_SYMBOL`'d for kernel modules only.
- `AT_EXECFD` (v5.8, `b8a61c9e7b4a`), `AT_FLAGS_PRESERVE_ARGV0` (v5.12,
  `2347961b11d4`) and `AT_FLAGS_TRANSPARENT_INTERP` (7.3, `b0f09c07966b`)
  are the auxv entries a loader can rely on.
- `PR_SET_MM_MAP` (Linux 3.18, needs `CONFIG_CHECKPOINT_RESTORE`) is how a
  transparent interpreter retargets `AT_PHDR`/`AT_ENTRY`/`AT_BASE`.

### 1.2 `binfmt_misc`, and the finding that matters

The mechanism is a **byte-offset magic matcher with an AND-mask**. On a
stable kernel that is all it is.

**The finding: since Linux 6.7, `binfmt_misc` can be mounted inside a user
namespace** (`21ca59b365c0`, "binfmt_misc: enable sandboxed mounts",
`FS_USERNS_MOUNT` + a superblock keyed per user namespace).

That is the whole game. It means:

```
unshare -Ur -m sh -c 'mount -t binfmt_misc binfmt_misc /proc/sys/fs/binfmt_misc'
```

registers format dispatch **as an unprivileged user**, in a superblock that
**disappears when the namespace exits**.

**Verified empirically on the reference machine** (not taken from the docs):

```
$ unshare -Ur -m sh -c 'mount -t binfmt_misc binfmt_misc /proc/sys/fs/binfmt_misc; echo OK'
OK
$ ./target/debug/tahoe-compat binfmt self-test
registering in a throwaway user namespace:
  :tahoe-selftest:M:0:\xcf\xfa\xed\xfe::/bin/echo:P
exit status: exit status: 0
stdout: /tmp/tahoe-compat-selftest/payload.bin /tmp/tahoe-compat-selftest/payload.bin
OK: the kernel dispatched the file to our interpreter.
```

Two occurrences of the path is correct, not a bug: `P` appends the original
`argv[0]`, so `argv` is `[/bin/echo, <binary>, <binary>]`.

The host's `/proc/sys/fs/binfmt_misc` was unchanged before and after — there
is a test for that (`a_rule_registered_in_a_namespace_does_not_escape_it`).

### 1.3 What `binfmt_misc` cannot do, and when that changes

| Capability | 6.18 (our baseline) | Linux 7.3 |
|---|---|---|
| Predicate on a *parsed* header field | impossible | ✅ BPF `match()` reads arbitrary offsets |
| Compute the interpreter per binary | impossible | ✅ `bpf_binprm_set_interp()` |
| Pass an argument to the interpreter | impossible | ✅ `bpf_binprm_set_interp_arg()` |
| Replace `PT_INTERP` with **no extra process** | impossible | ✅ the `L` flag |
| Full `/proc/pid/exe` transparency | impossible | ✅ the `T` flag |
| Multi-file coordination | impossible | still userspace |

The 7.3 BPF handler (`CONFIG_BINFMT_MISC_BPF`, `struct_ops` with sleepable
`match`/`load` programs, `bpf_dynptr_from_file()`) is the sanctioned upstream
answer to "I need semantic dispatch". Loading such a handler requires
`CAP_BPF`, i.e. a **BPF token** for an unprivileged namespace — so the
rootless story is weaker there than for classic entries.

**Is `PE\0\0` reachable?** The precise answer matters and is usually stated
wrong. `PE\0\0` sits at `offset_of_pe_header = e_lfanew`. With a 256-byte
window:

| `e_lfanew` | 0x80 (common, MSVC) | 0xF8 | 0xFC | 0x100+ |
|---|---|---|---|---|
| `PE\0\0` matchable | **yes** | **yes** | yes (last valid) | no |

So it is reachable for *many* PE files but not all. A `PE\0\0` rule would
silently match a subset, which is worse than an honest `MZ` rule that
matches all of them and lets userspace sort the rest out. `MZ` is what
Arch's own `wine` package registers, and what Tahoe keeps.

### 1.4 Related mechanisms, and why they were rejected

| Mechanism | Verdict |
|---|---|
| `memfd_create` + `fexecve` | Executing from memory works (`execveat` + `AT_EMPTY_PATH`), but adds nothing over a path and interacts awkwardly with `MFD_NOEXEC_SEAL` / `vm.memfd_noexec`. **Rejected: no benefit.** |
| `ptrace` loaders | `PTRACE_O_TRACEEXEC` cannot substitute the image being executed. Wine uses ptrace only for its *debugger* APIs, and `userfaultfd` (not ptrace) for `PAGE_GUARD` semantics. **Rejected: cannot do the job.** |
| `SECCOMP_USER_NOTIF` | Round-trips every intercepted syscall through a supervisor. Documented as *not* a security boundary, with an inherent TOCTOU on `CONTINUE`. Real users apply it per syscall, not per process. **Rejected for hot-path syscall translation.** |
| `PR_SET_SYSCALL_USER_DISPATCH` | The kernel documentation names Wine as the motivating consumer. But it is **x86-only upstream in 7.3** (arm64 does not select `ARCH_SUPPORTS_SYSCALL_USER_DISPATCH`) and **no Wine code uses it today**. **Rejected for now: the consumer it was written for does not consume it.** |

---

## 2. Four architectures, compared

Scored against what a small distro can actually ship and maintain.

### Option A — A deeper Wine integration

- **What it means:** prefix management, discovery, desktop integration,
  runtime component provisioning, acceleration detection, and the
  `.desktop`/Dock plumbing, all built on unmodified upstream Wine.
- **Compatibility:** whatever Wine has. No more, no less.
- **Performance:** Wine's, minus a negligible launcher overhead.
- **Security:** process isolation per prefix; no new attack surface in the
  kernel; Wine's own sandboxing limits remain.
- **Complexity:** **low.** It is all userspace and all testable without Wine
  installed.
- **Update burden:** **none** — Wine updates freely, and a `wine-staging`
  user can swap in without Tahoe noticing.
- **Own code:** moderate, and it is code that makes the *user experience*
  better rather than duplicating an engine.

### Option B — A universal loader and format dispatcher

- **What it means:** one binary that identifies any format and loads the
  right runtime.
- **Reality check:** this is where the mission's warning applies. A
  *dispatcher* is a few thousand lines and entirely reasonable. A *loader*
  is not: Wine's PE loader plus `ntdll` thunks plus `wow64` is decades of
  work, and Darling's Mach-O loader plus dyld plus `darlingserver` is over a
  decade. Writing either means reimplementing the parts that are already
  correct.
- **Verdict:** the dispatcher half is Option C. The loader half is refused
  (ADR-0005, unchanged).

### Option C — Wine and Darling behind Tahoe's own services and launch paths

- **What it means:** Tahoe owns identification, planning, supervision,
  discovery and desktop integration; Wine and Darling stay unmodified and
  stay in their own processes.
- **Compatibility:** same as upstream, but **discoverable and honest**: the
  user sees a confidence and a caveat rather than a silent failure.
- **Performance:** one `execve` of overhead per launch, measured at ~1.5 ms
  (§4).
- **Security:** strictly better than A. The compatibility layer is a child
  process with a whitelisted environment, a dedicated prefix, its own
  process group that a timeout can kill wholesale, and no inherited secrets.
- **Complexity:** moderate.
- **Update burden:** none for Wine/Darling.
- **Own code:** the dispatcher, the supervisor, the parsers, the discovery.
  All testable without either runtime installed — which is what made 457
  tests possible on a machine with neither.

### Option D — Kernel patches or a kernel extension

- **What it means:** a new `linux_binfmt`, or patches to `binfmt_misc`.
- **What it would buy on 6.18:** semantic dispatch and per-binary
  interpreters — **both of which Linux 7.3 already ships upstream** (BPF
  `match`/`load`).
- **What it would buy beyond 7.3:** multi-file coordination, which BPF
  deliberately does not do and which a wrapper interpreter does correctly.
- **Cost:** a permanent out-of-tree patch against a distribution that
  updates its kernel continuously; a new attack surface in `fs/exec.c` with
  a history of CVEs (eight in the binfmt area in 2022–2026, e.g.
  CVE-2026-68186, CVE-2026-74483…74487); and no ability to upstream it,
  because the upstream answer already exists.
- **Verdict: rejected.** Not because it is impossible, but because it would
  reimplement, worse and unserialisable, something that is already merged.
  A detailed treatment is in `kernel-integration.md`.

### Decision

**Option C, built on the existing UEL.** With two results from Option D
folded in, because they are free:

1. **Rootless namespaced `binfmt_misc`** (6.7+) gives real kernel-level
   dispatch with no root and no host mutation. Shipped and tested.
2. **The 7.3 feature set is a roadmap item, not a dependency** — when the
   distribution's kernel reaches it, `L` and `T` remove the extra process
   and the `/proc/pid/exe` lie, and `B` replaces the fat-Mach-O ambiguity
   workaround with a real predicate. Until then, Tahoe registers only the
   classic `M`/`P` entries that work everywhere.

Nothing in Option D was implemented as a kernel patch. Nothing needed to be.

---

## 3. What Wine actually provides (verified)

From `gitlab.winehq.org/wine/wine` (master and the `wine-11.0` tag):

- **Wine 11.0** (2026-01-13) completes the **new WoW64**: 32-bit PE thunks
  into a 64-bit process through `wow64.dll`/`wow64win.dll`/`wow64cpu.dll`.
  `WINEARCH=win32` is deprecated and unsupported in that mode; `wine64` is
  gone.
- **`winemenubuilder`** is the desktop-integration mechanism Tahoe uses
  rather than replaces. It writes
  `$XDG_DATA_HOME/applications/wine/**/*.desktop`, MIME packages, menus and
  PNG icons, and it emits **`StartupWMClass`** — exactly the key a Wayland
  compositor needs to associate a window with its dock entry. It is invoked
  from `wine.inf`'s `RunServices` and from `shell32` on shortcut changes.
  Disabling it is a documented registry override
  (`HKCU\Software\Wine\DllOverrides`), and Lutris does exactly that.
- **Installed applications** are discoverable from
  `drive_c/users/<user>/AppData/Roaming/Microsoft/Windows/Start Menu/Programs`
  and `drive_c/ProgramData/Microsoft/Windows/Start Menu/Programs`, with the
  legacy `users/<user>/Start Menu/Programs` still produced by older
  prefixes. Bottles globs the same three paths.
- **Launching a discovered app** goes through `wine start`, which resolves a
  `.lnk` the way Windows does. `wine start /wait` returns the child's exit
  code and assigns it to a kill-on-close job object.
- **NTSync** (`CONFIG_NTSYNC`, Linux **6.14**, `/dev/ntsync`) is used
  **automatically** by Wine 11 — there is no environment variable. Fallback
  is the classic wineserver sync path.
- **`WINEESYNC` and `WINEFSYNC` do not exist in upstream Wine.** They are
  Proton/wine-staging only. Tahoe does not advertise them, and there is a
  test asserting the list does not contain them.
- **The Wayland driver is real** (`dlls/winewayland.drv/`) but **X11 has
  precedence** in the built-in list `mac,x11,wayland`, and Wine 11.0's own
  release notes still call it experimental.
- **vkd3d is upstream; DXVK is not.** Wine ships `libs/vkd3d`; DXVK is a
  separate zlib-licensed project that Proton vendors.
- **Gecko and Mono** are looked up in `/usr/share/wine/{gecko,mono}/` and
  five other places before any download is attempted. Shipping the MSIs
  there prevents the download dialog.

Wine's own architecture document settles the honesty question:

> "Wine is not a virtual machine, it does not emulate a CPU, and you are not
> supposed to install Windows nor any Windows device drivers on top of it;
> rather, Wine is an implementation of the Windows API."
> — `Wine-Developer's-Guide/Architecture-Overview`

There is no `ntoskrnl`, no HAL, no kernel-mode drivers, no WDDM, no
DirectStorage, and kernel-mode anti-cheat cannot load.

## 4. What Darling actually provides (verified)

From `github.com/darlinghq/darling` and the `darlingserver` repository:

- **`mldr`** maps the executable and whatever `LC_LOAD_DYLINKER` names, then
  jumps to *its* entry point. It **bootstraps dyld; it does not bypass it.**
  Darling's dyld is a fork of Apple's.
- **`darlingserver`** is a userspace "kernel" reached over
  `$DPREFIX/.darlingserver.sock`, implementing ~80 Mach and BSD calls from a
  vendored XNU fork. It replaced the kernel module in **July 2022**
  (`v0.1.20220704`: "ditches the kernel module in favor of a
  fully-userspace-based implementation").
- **The prefix** is an overlayfs merge of the read-only install tree and
  `~/.darling`. This is why the README warns that prefixes cannot live on
  NFS or an encrypted home directory.
- **`darling` has no `--scratch-dir` or any option beyond `--help` and
  `--version`.** Its grammar is `darling <program> [args]`,
  `darling shell`, `darling exec`, `darling shutdown`, and `DPREFIX` is the
  only environment variable.
- **Darling does register binfmt entries** —
  `src/startup/mldr/darling.conf.in` contains `:Mach-O-64-bit:M::\xcf\xfa\xed\xfe::…/mldr:P`.
  **But the path is inconsistent in-tree**: CMake installs `mldr` to
  `libexec/darling/usr/libexec/darling/mldr`, while the template points at
  `libexec/darling/bin/mldr`, and nothing in the tree creates `bin/mldr`.
  **A distro must fix or supply that symlink**, and Tahoe's own rules avoid
  the problem by pointing at its own dispatch helper instead.
- **Capability, in the maintainers' own words:** "many CLI tools already
  functional. GUI application support is in active development."
  `mldr.c` contains `#error TODO: aarch64`; the ARM64 PR (#1753) is open and
  the maintainer has said he will not merge it. Xcode does not run
  (README). `Combine` and `CryptoKit` are missing; SwiftUI is a stub. The
  only CI workflow builds DEBs and **never executes a Mach-O**, so there is
  no automated end-to-end evidence at all.
- **Darling is the only maintained Mach-O execution environment on Linux.**
  `maloader` is dormant since 2022. Everything else in that space
  (`osxcross`, `cctools-port`) is a *toolchain* that cross-compiles Mach-O
  and cannot run it.

## 5. What the containers need (verified)

| Format | Structure | Linux tools | Verdict |
|---|---|---|---|
| `.dmg` | UDIF: 512-byte big-endian `koly` trailer, XML plist with base64 `blkx` tables, chunk methods ADC/zlib/bzip2/**LZFSE**/LZMA | 7-Zip (LGPL) reads all of them; `libdmg-hfsplus` and `dmg2img` (both GPL) lack LZFSE | parse the trailer ourselves; hand extraction to a tool rather than reimplement a decompressor |
| HFS+ | in payload | **in-kernel, read-write** (`fs/hfsplus`) | mount, do not extract |
| APFS | in modern payloads | **not in mainline**; `apfs-fuse` (GPL-2.0, read-only) or 7-Zip | read-only only; **UNVERIFIED** that "typical modern DMGs are APFS" |
| `.pkg` | XAR; `Payload` is cpio, gzip- or pbzx-wrapped | `xar` (BSD-3), libarchive (BSD-2), 7-Zip | unpack; never execute |
| `.xip` | XAR with a `Content` entry that is a pbzx stream of XZ chunks → cpio | Darling's `unxip` | same |
| Info.plist | XML or `bplist00` | `libplist` (LGPL-2.1), Python `plistlib` | parse ourselves — the format is small and the alternative is a dependency |
| `.app` | directory; `Contents/MacOS/<CFBundleExecutable>` | — | resolve, never `execve` the directory |

### The `0xCAFEBABE` collision

`FAT_MAGIC` is also the Java class file magic. `binfmt_misc` **cannot**
express "and not a class file": `nfat_arch` sits at offset 4 and resolving
the collision requires reading it, which a mask cannot do. `file(1)` solves
it by range (a Java class stores its major version there, and no universal
binary has ~20 architectures). `lipo` does not check at all.

Tahoe does three things: `tahoe-format` applies `file(1)`'s heuristic **plus
a validity check on each slice's magic**; the fat `binfmt_misc` rule is
registered knowing its interpreter must re-check; and there is a test pinning
the reason.

---

## 6. Licensing

| Component | License | How Tahoe uses it | Constraint |
|---|---|---|---|
| Wine | LGPL-2.1-or-later | separate process | none; Tahoe links nothing |
| Darling | GPL-3.0-or-later | separate process | must ship its license; must not link it in |
| `darlingserver` | no SPDX file in-tree; headers are GPL | separate process | treat as GPL-3 |
| DXVK | zlib | not used | — |
| vkd3d-proton | LGPL-2.1 | not used | — |
| 7-Zip | LGPL (RAR parts are unRAR-licensed) | optional external tool | do not bundle for RAR |
| `apfs-fuse` | GPL-2.0 | optional external tool | separate process |
| `xar` (mackyle) | BSD-3-Clause | optional external tool | none |
| `libplist` | LGPL-2.1 | **not used** — we parse in Rust | none |
| Linux kernel | GPL-2.0 | unmodified | no patches shipped |

`LICENSING.md` §3 already forbids linking GPL code into a Tahoe binary. The
process boundary is what makes `tahoe-compat` able to be `MIT OR
Apache-2.0` while driving a GPL-3 runtime.

---

## 7. Measured overhead

From `tahoe-bench` on the reference machine (release build, 20 000
micro-iterations, 40 spawn iterations):

| Operation | Mean | p95 |
|---|---|---|
| `probe_bytes` ELF | 92 ns | 92 ns |
| `probe_bytes` PE | 88 ns | 91 ns |
| `probe_bytes` Mach-O | 78 ns | 80 ns |
| `probe_bytes` universal binary | 192 ns | 197 ns |
| `decide` + command construction | 242–333 ns | 274–363 ns |
| build 4 `binfmt_misc` register strings | 5.8 µs | 6.1 µs |
| bare `fork`+`exec` of `/bin/true` | 1.11 ms | 1.27 ms |
| through the supervisor | 2.66 ms | 2.78 ms |

**Supervisor overhead: ~1.5 ms per launch.** Identification and planning are
~300 ns, i.e. four orders of magnitude below the fork itself, and therefore
not worth optimising.

The first implementation used a fixed 5 ms poll and cost **4.7 ms** per
launch (444% of bare). The harness found it; moving the `wait()` into a
thread (where the kernel wakes it on exit) brought it to ~1.5 ms. There is
an honest note in the harness that disabling output capture was expected to
help and measurably did **not**.

**Not measured:** anything Wine or Darling does. Reporting a "speedup" for
code we did not write would be dishonest.

---

## 8. Conclusion

The best architecture for Project Tahoe is **Option C**, and the research
says the ambitious-sounding options are the *worse* ones:

- A **custom loader** means reimplementing Wine's PE loader or Darling's
  dyld — the parts that already work.
- A **kernel patch** means reimplementing, out-of-tree and unmaintainable,
  the semantic dispatch that Linux 7.3 already merged.
- **Forking Wine** means owning Wine's compatibility forever, for a problem
  the user experiences as "the Dock entry is missing".

What is left, and what Tahoe built, is the part that is genuinely missing
from the ecosystem: honest identification, a confidence report, real
discovery of installed applications, a supervisor that will not orphan
processes, rootless kernel-level dispatch, and parsers for the containers
nobody else bothers to describe.
