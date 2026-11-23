#!/bin/sh
# Boot-partition safety for package operations on the device (lavender: postmarketOS
# boot-deploy runs with deviceinfo_flash_kernel_on_update="true", so every mkinitfs trigger
# — any package touching /usr/lib/udev, /lib/firmware, kernel modules, … — regenerates
# /boot/boot.img AND dd's it to the active `boot` partition).
#
#   scripts/boot-backup.sh backup   copy the active boot partition + /boot + versions to the
#                                   HOST (default ~/pocket-os-device-backups/<host>/<ts>),
#                                   verify hashes on both sides
#   scripts/boot-backup.sh verify   after a package operation: is the boot partition still
#                                   the image boot-deploy generated, and does it match the
#                                   last backup? (prints kernel/package versions)
#
# Needs root on the device (sudo). POCKET_SUDO_PASSWORD_FILE=<file> feeds sudo from a file;
# otherwise ssh -t prompts. Backups never go into the repository.
# Restore (only if the device no longer boots; see docs/development/README.md §5):
#   phone in fastboot mode (power off, hold Volume-down + Power), then on the host
#   fastboot flash boot <backup>/boot-partition.img && fastboot reboot
set -eu
HOST="${POCKET_DEVICE:-lavender}"
dest_root="${POCKET_BACKUP_DIR:-$HOME/pocket-os-device-backups/$HOST}"
part=/dev/disk/by-partlabel/boot

root_sh() { # run a root shell script on the device
	if [ -n "${POCKET_SUDO_PASSWORD_FILE:-}" ]; then
		ssh "$HOST" "sudo -S -p '' sh -euc '$1'" <"$POCKET_SUDO_PASSWORD_FILE"
	else
		ssh -t "$HOST" "sudo sh -euc '$1'"
	fi
}
user="$(ssh "$HOST" id -un)"

case "${1:-}" in
backup)
	dest="$dest_root/$(date +%Y%m%d-%H%M%S)"
	mkdir -p "$dest"
	chmod 700 "$dest_root"
	# Stage on the device (tmpfs, user-readable) so an interactive sudo prompt does not mix
	# with the binary stream.
	root_sh "
		t=/tmp/pocket-boot-backup; rm -rf \$t; mkdir -m 700 \$t
		dd if=$part of=\$t/boot-partition.img bs=1M status=none
		tar -C / -cf \$t/boot-dir.tar --exclude=boot/lost+found boot
		{ echo \"partition: \$(readlink -f $part) \$(blockdev --getsize64 $part) bytes\"
		  echo \"kernel: \$(uname -r)\"; cat /proc/cmdline; echo
		  apk info -v 2>/dev/null | grep -E \"^(linux-postmarketos|device-|postmarketos-mkinitfs|boot-deploy|firmware-)\"
		  grep -E \"^deviceinfo_(flash_method|flash_kernel_on_update|generate_bootimg)\" /usr/share/deviceinfo/deviceinfo
		} > \$t/versions.txt
		n=\$(stat -c %s /boot/boot.img)
		{ sha256sum $part | sed \"s|$part|device:boot-partition|\"
		  head -c \$n $part | sha256sum | sed \"s|-|device:boot-partition-prefix|\"
		  sha256sum /boot/boot.img | sed \"s|/boot/boot.img|device:/boot/boot.img|\"
		} > \$t/device-sha256.txt
		chown -R $user \$t"
	scp -q "$HOST:/tmp/pocket-boot-backup/*" "$dest/"
	ssh "$HOST" 'rm -rf /tmp/pocket-boot-backup' 2>/dev/null || root_sh 'rm -rf /tmp/pocket-boot-backup'
	(cd "$dest" && sha256sum boot-partition.img boot-dir.tar >SHA256SUMS)
	dev_part=$(awk '/device:boot-partition$/ {print $1}' "$dest/device-sha256.txt")
	host_part=$(awk '/boot-partition.img/ {print $1}' "$dest/SHA256SUMS")
	prefix=$(awk '/device:boot-partition-prefix/ {print $1}' "$dest/device-sha256.txt")
	bootimg=$(awk '/device:\/boot\/boot.img/ {print $1}' "$dest/device-sha256.txt")
	[ "$dev_part" = "$host_part" ] || { echo "FAIL: host copy differs from the partition" >&2; exit 1; }
	[ "$(head -c 8 "$dest/boot-partition.img")" = "ANDROID!" ] || { echo "FAIL: no Android boot header" >&2; exit 1; }
	echo "OK: $dest"
	echo "    boot partition sha256 $host_part (verified on device and host)"
	if [ "$prefix" = "$bootimg" ]; then
		echo "    the partition contains /boot/boot.img ($bootimg)"
	else
		echo "    NOTE: the partition does NOT start with /boot/boot.img (flashed image differs from the generated one)"
	fi
	;;
verify)
	last=$(find "$dest_root" -mindepth 2 -maxdepth 2 -name SHA256SUMS 2>/dev/null | sort | tail -n1)
	ref=$( [ -n "$last" ] && awk '/boot-partition/ {print $1}' "$last" || true)
	out=$(root_sh "
		n=\$(stat -c %s /boot/boot.img)
		echo \"kernel \$(uname -r)\"
		echo \"partition \$(sha256sum $part | cut -d\" \" -f1)\"
		echo \"prefix \$(head -c \$n $part | sha256sum | cut -d\" \" -f1)\"
		echo \"bootimg \$(sha256sum /boot/boot.img | cut -d\" \" -f1)\"
		echo \"generated \$(date -r /boot/boot.img +%F_%T)\"")
	echo "$out" | tr -d '\r'
	p=$(echo "$out" | awk '/^partition/ {print $2}' | tr -d '\r')
	[ "$(echo "$out" | awk '/^prefix/ {print $2}')" = "$(echo "$out" | awk '/^bootimg/ {print $2}')" ] &&
		echo "OK: the boot partition holds the generated /boot/boot.img" ||
		echo "WARN: the boot partition does not hold /boot/boot.img"
	if [ -n "$ref" ]; then
		[ "$p" = "$ref" ] && echo "OK: identical to the last backup ($(dirname "$last"))" ||
			echo "CHANGED since the last backup ($(dirname "$last")): reboot once and confirm, then take a new backup"
	else
		echo "no backup on this host yet: scripts/boot-backup.sh backup"
	fi
	;;
*)
	sed -n '2,19s/^# \{0,1\}//p' "$0" >&2
	exit 2
	;;
esac
