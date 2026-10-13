# binfmt.d — entries Tahoe ships
#
# INSTALLED to /usr/lib/binfmt.d/ by the tahoe-base package and applied by
# systemd-binfmt.service. Registration requires root and is NOT done by
# `cargo build` / `cargo test`.
#
# Shipped:
# - wine.conf — `:DOSWin:M::MZ::/usr/bin/wine:F`, mirroring Arch's own
#   /usr/lib/binfmt.d/wine.conf (plus the F flag so the interpreter is
#   pre-opened at registration and survives mount-namespace changes).
#
# Deliberately NOT shipped:
# - Mach-O (`cf fa ed fe`): Darling's documented invocation is
#   `darling <binary>`; no upstream binfmt registration exists, and a
#   byte-magic entry cannot express the fat-vs-Java-class ambiguity
#   (ADR-0007). Mach-O goes through `tahoe-uel` explicitly.
# - ELF arch dispatch: the kernel already executes native ELF; foreign-arch
#   ELF needs a userspace decision (which emulator? which rootfs?), not a
#   magic line.
