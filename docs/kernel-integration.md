# Kernel integration: what is justified, and what is not

> Phase 5 deliverable. Answers the required questions for every candidate
> change, and records the negative results, which are as important as the
> positive ones.
>
> Verified on kernel `6.18.55-1-cachyos-lts`, x86_64, 2026-10-14.

---

## 1. Short answer

**No kernel patch is justified for Project Tahoe.**

Not because kernel work is hard, but because the two things a patch would
buy are **already upstream**: namespaced `binfmt_misc` mounts since 6.7, and
semantic dispatch via BPF-backed handlers since 7.3. Adding a third
mechanism out-of-tree would be a permanent maintenance cost against a
continuously-updating distribution, for zero capability that upstream does
not already provide or have a plan for.

What Tahoe ships instead is in §4: a rootless, namespaced, host-safe
dispatcher built entirely on the stock kernel, with tests.

---

## 2. Candidate changes, and the required questions

### 2.1 A `linux_binfmt` userspace registration API

| Question | Answer |
|---|---|
| What problem? | Register a format handler without `binfmt_misc`'s byte-mask limits. |
| What is impossible today? | True semantic predicates and per-binary interpreter selection — **but only on kernels before 7.3.** |
| How would it work? | A syscall taking a handler description and a callback. |
| Security | A new reference from the format list into a task's address space; the binfmt area has had **eight CVEs in 2022–2026**. |
| On kernel update | Out-of-tree patch against `fs/exec.c`. Breaks on every internal change. |
| How to prove it helps | It would not: Linux 7.3's BPF `struct_ops` is the upstream design for exactly this, and the maintainers chose it deliberately ("struct_ops is the sanctioned mechanism for this kind of user-supplied policy callback", `b4bfe2f6b011`). |
| **Verdict** | **Rejected.** Upstream already answered it. |

### 2.2 A patch to `binfmt_misc` for per-architecture ELF dispatch

| Question | Answer |
|---|---|
| What problem? | `binfmt_misc` cannot express "ELF with `e_machine == AArch64`". |
| Impossible today? | On a *stable* kernel, yes. On 7.3, a `B` handler reads the file at arbitrary offsets and answers it. |
| Note | The kernel's own `:i386:` example already does byte-mask field matching at fixed offsets (`\x02\x00\x3e\x00` at 16). It is ugly but it works for one fixed value. |
| Security | No new surface: same mechanism, more masks. |
| On kernel update | Patch rot. |
| How to prove it helps | A userspace wrapper already does this better: it can read the *whole* header, check `PT_INTERP`, consult `$ORIGIN` and sidecar config, and report a confidence. |
| **Verdict** | **Rejected: a wrapper interpreter solves it, and solves more of it.** |

### 2.3 Honest loaders in the kernel (`binfmt_pe`, `binfmt_macho`)

| Question | Answer |
|---|---|
| What problem? | Loading PE/Mach-O without a userspace process. |
| Impossible today? | Nothing — `binfmt_misc` can already redirect the exec. |
| Security | Loading a foreign binary format in kernel space, with a parser written in C in ring 0. This is the worst possible place for a parser. |
| **Verdict** | **Rejected outright.** A PE loader is not a dispatch problem; it is `ntdll` thunks, TLS callbacks, SEH, and a Windows-ABI stack. None of that belongs in the kernel. The mission's own rule — "Не помещай сложные Windows API или Darwin frameworks в ядро" — is exactly right. |

### 2.4 An experimental Tahoe kernel package

| Question | Answer |
|---|---|
| What problem? | Ship 7.3's `T`/`L`/`B` flags to users of a 6.18 LTS kernel. |
| Technically how | Backport the ~20-commit binfmt series and build a `linux-tahoe` package. |
| Security | The series carries CVE-2026-68186, CVE-2026-74483…74487 fixes. A backport must include them. |
| On kernel update | The package is per-kernel-version; every upstream stable bump is a rebase. |
| How to prove it helps | Measurably: `L` removes one `execve` from native-ELF-with-`PT_INTERP` dispatch, and `T` fixes `/proc/pid/exe` identity. Both are real. |
| **Verdict** | **Deferred, not rejected.** This is the one kernel-adjacent option with a real, measurable benefit and a bounded cost. It is listed in the roadmap as a *later* option, to be attempted only after the userspace path is complete and only with a written design. The distribution's kernel will reach 7.3 by normal upgrade, which makes the backport a temporary convenience rather than a requirement. |

---

## 3. What is impossible without a kernel change

Very little, and all of it is cosmetic or coordination:

1. **Semantic dispatch on a pre-7.3 kernel** — solved by a wrapper
   interpreter at the cost of one extra `execve` (~1 ms).
2. **Full `proc` identity for translated binaries** on a pre-7.3 kernel —
   solved by `T` in 7.3, and otherwise unfixable. Users can see a Wine
   process as a Wine process. That is arguably *correct*.
3. **Multi-file coordination** — not a kernel job at all.
4. **Loading a foreign binary format natively** — refused on principle.

---

## 4. What Tahoe ships instead

### 4.1 Rootless, namespaced dispatch

`tahoe-compat/src/binfmt.rs` builds register strings, validates every
constraint the kernel enforces (window, mask length, absolute interpreter,
name, 1920-byte limit), and registers them inside
`unshare -Ur -m`.

Verified behaviour:
- registration succeeds with no root;
- the kernel dispatches a real file to the interpreter;
- the entry is visible inside its namespace and **absent outside it**;
- the host's dispatcher is byte-for-byte unchanged afterwards.

Both properties have tests.

### 4.2 Static `binfmt.d` for the system-wide cases

For a double-click in a file manager, a system-wide entry is needed. Those
are shipped as files in `binfmt.d/` and applied by `systemd-binfmt`, exactly
as Arch's own `wine` package does. They are **not** installed by `cargo
build` or `cargo test`: installing them needs root, and the tests must not.

### 4.3 The rules Tahoe registers, and what each one admits

| Rule | Magic | Known limitation |
|---|---|---|
| `tahoe-wine-pe` | `MZ` at 0 | `MZ` is also DOS, NE and OS/2. Wine sorts that out; Tahoe does not pretend to. |
| `tahoe-macho64` | `cf fa ed fe` | — |
| `tahoe-macho32` | `ce fa ed fe` | — |
| `tahoe-macho-fat` | `ca fe ba be` | **Also the Java class magic.** The interpreter re-checks `nfat_arch`; the kernel cannot. |

`PE\0\0` is deliberately **not** registered. It sits at `e_lfanew`, and
while 0x80 and 0xF8 are inside the 256-byte prefetch window, 0xFC is the
last that is and 0x100+ is outside it. A rule that matches a *subset* of PE
files silently would be worse than one that matches all of them.

No rule matches ELF. A test enforces that: shadowing the native loader for
every binary on the system would be catastrophic.

### 4.4 When the kernel gains 7.3's features

| Feature | What Tahoe will change |
|---|---|
| `L` (loader substitution) | Native-ELF-with-`PT_INTERP` dispatch loses its extra process. Useful for the emulator path. |
| `T` (transparent) | `/proc/pid/exe` stops lying about Wine processes. |
| `B` (BPF `match`) | The fat-Mach-O/Java ambiguity becomes a real predicate instead of an interpreter re-check. |
| `max_binfmt_misc_interpreters` | A new accounting limit to respect; `tahoe-compat` already validates against the 100-interpreter-per-entry bound. |

None of these is a dependency. `tahoe-compat` emits only `P`, `O`, `C` and
`F`, because emitting a flag an older kernel does not parse makes the whole
registration fail, and a silently ignored flag is worse.

---

## 5. Safety properties of the current design

- No kernel modification, now or planned for the shipping configuration.
- No host mutation: a namespaced superblock is keyed per user namespace and
  dies with it.
- No privileged operation in the normal path.
- The `C` flag (credentials from the binary) is **off by default**; the
  kernel documentation warns it runs the interpreter with the binary's
  privileges. A caller must opt in deliberately.
- The `O` flag (pass the binary as a file descriptor, so the interpreter can
  read a file it could not otherwise open) is off by default for the same
  reason: it widens what the interpreter can reach.
- Every rule is validated before it reaches the kernel, so a failure is a
  typed error rather than a string.
