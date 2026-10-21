# SPDX-License-Identifier: GPL-3.0-or-later
# Tahoe archiso profile. Derived from archiso's releng profile patterns;
# profile format documented at https://wiki.archlinux.org/title/Archiso.
# The live session is Hyprland + MacShell (see docs/iso-session.md).
# STATUS: written, NOT YET BUILT (needs `archiso`; iso/build.sh drives it).

iso_name="tahoe"
iso_label="TAHOE_$(date --date="@${SOURCE_DATE_EPOCH:-$(date +%s)}" +%Y%m)"
iso_publisher="Project Tahoe <https://github.com/iboshka24/project-tahoe>"
iso_application="Project Tahoe Live (Hyprland + MacShell desktop)"
iso_version="$(date --date="@${SOURCE_DATE_EPOCH:-$(date +%s)}" +%Y.%m.%d)"
install_dir="tahoe"
buildmodes=('iso')
bootmodes=('bios.syslinux' 'uefi.systemd-boot')
arch="x86_64"
pacman_conf="pacman.conf"
airootfs_image_type="squashfs"
airootfs_image_tool_options=('-comp' 'xz' '-Xbcj' 'x86' '-b' '1M' '-Xdict-size' '1M')
file_permissions=(
  ["/etc/shadow"]="0:0:400"
  ["/etc/sudoers.d/10-tahoe-live"]="0:0:440"
  ["/root"]="0:0:750"
  # mkarchiso copies airootfs without preserving modes, so a script that ships
  # as 0755 in the repository is 0644 in the image unless it is listed here.
  # macshell-start escaped this only because the session runs it as `sh ...`.
  ["/usr/local/bin/tahoe-installer"]="0:0:755"
  ["/usr/local/bin/swaybg"]="0:0:755"
)
