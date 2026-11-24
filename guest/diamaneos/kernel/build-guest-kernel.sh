#!/bin/bash
# Copyright (C) 2026 DiamaneOS
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#      http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#
# build-guest-kernel.sh: builds arm64/Image, the guest kernel of the DiamaneOS VMs (README.md).
#
#   build-guest-kernel.sh --kernel DIR --aosp DIR --out DIR [--jobs N] [--new]
#   build-guest-kernel.sh --kernel DIR --clang DIR --build-tools DIR --out DIR [--jobs N] [--new]
#
# --kernel       a Git checkout of kernel_common-6.18 at the commit PIN names, with no local changes
# --aosp         an Android source tree: the clang and build tool prebuilts are taken from it
# --clang        or the clang prebuilt itself (prebuilts/clang/host/linux-x86/clang-r596125)
# --build-tools  and the build tool prebuilt (prebuilts/build-tools: bison, flex, m4, bc)
# --out          an empty or new folder for the build. Image, guest.config and the config check land in it.
# --new          a new guest kernel from another commit: the differences are shown, not refused
#
# The configuration is microdroid_defconfig plus the options for SD cards and USB drives, without what trim.list
# lists, with two additional boolean hardening options. check-guest-config.py refuses hardening downgrades and every option the VMs
# need must be set. Image is built with fixed build stamps, so the same source and toolchain give the same bytes.
#
# Without --new the build must give the published files: a checkout at another commit, a guest.config or an Image
# that differs from the published one fails the script.
set -euo pipefail

# The kernel_common commit of the published arm64/Image (README.md).
PIN=040efa0b976713b296af67235d38df55dd4b32b6

usage() { sed -n '17,32p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }
KERNEL= AOSP= CLANG= TOOLS= OUT= JOBS= NEW=0
while [ $# -gt 0 ]; do
  case $1 in
    --new) NEW=1; shift; continue ;;
    --kernel) KERNEL=$2 ;; --aosp) AOSP=$2 ;; --clang) CLANG=$2 ;; --build-tools) TOOLS=$2 ;;
    --out) OUT=$2 ;; --jobs) JOBS=$2 ;; *) usage ;;
  esac
  shift 2
done
JOBS=${JOBS:-$(nproc)}
# A difference from the published files: refused, unless this is a new guest kernel (--new).
differs() {
  if [ $NEW = 1 ]; then echo "$1"; else echo "FAILED: $1" >&2; exit 1; fi
}
[ -n "$KERNEL" ] && [ -n "$OUT" ] || usage
if [ -n "$AOSP" ]; then
  CLANG=${CLANG:-$AOSP/prebuilts/clang/host/linux-x86/clang-r596125}
  TOOLS=${TOOLS:-$AOSP/prebuilts/build-tools}
fi
[ -x "$CLANG/bin/clang" ] || { echo "no clang in $CLANG" >&2; exit 1; }
for t in bison flex m4 gavinhoward-bc; do
  [ -x "$TOOLS/linux-x86/bin/$t" ] || { echo "no $t in $TOOLS/linux-x86/bin" >&2; exit 1; }
done
HERE=$(cd "$(dirname "$0")" && pwd)
KERNEL=$(cd "$KERNEL" && pwd)
mkdir -p "$OUT"; OUT=$(cd "$OUT" && pwd)

# The release string carries the commit (-g...), and a changed tree would add -dirty: both change the Image.
[ -z "$(git -C "$KERNEL" status --porcelain --untracked-files=no)" ] || { echo "the kernel checkout has local changes" >&2; exit 1; }
commit=$(git -C "$KERNEL" rev-parse HEAD)
echo "kernel_common $commit"
[ "$commit" = "$PIN" ] || differs "kernel_common $commit is not the published Image's commit $PIN"

# The build tools: clang first, bc under its usual name, bison's data and m4 from the same prebuilt.
mkdir -p "$OUT/hostbin"
ln -sf "$TOOLS/linux-x86/bin/gavinhoward-bc" "$OUT/hostbin/bc"
export PATH="$CLANG/bin:$PATH:$TOOLS/linux-x86/bin:$OUT/hostbin"
export BISON_PKGDATADIR="$TOOLS/common/bison" M4="$TOOLS/linux-x86/bin/m4"
# Fixed build stamps: the commit's date, and the user, host and build number the published Image has. The date is in
# ISO form because the kernel also hands it to `date -d` for the built-in initramfs, and `date -d` does not read Git's
# default form: the initramfs would then carry the time of the build.
export KBUILD_BUILD_TIMESTAMP="$(git -C "$KERNEL" log -1 --format=%cd --date=iso HEAD)"
date -d "$KBUILD_BUILD_TIMESTAMP" > /dev/null || { echo "date -d does not read $KBUILD_BUILD_TIMESTAMP" >&2; exit 1; }
export KBUILD_BUILD_USER=diamaneos-build KBUILD_BUILD_HOST=diamaneos-builder KBUILD_BUILD_VERSION=1
CFG="/bin/bash $KERNEL/scripts/config"
MK="make -C $KERNEL ARCH=arm64 LLVM=1 LLVM_IAS=1 -j$JOBS"

# 1. The base: microdroid_defconfig with filesystem support and the guest's extra hardening, without modules.
rm -rf "$OUT/base" "$OUT/build"
$MK O="$OUT/base" microdroid_defconfig > "$OUT/build.log" 2>&1
$CFG --file "$OUT/base/.config" -e SECCOMP -e SECCOMP_FILTER -e FAT_FS -e VFAT_FS -e EXFAT_FS \
  -e MSDOS_PARTITION -e EFI_PARTITION -e DEVTMPFS -e NLS -e NLS_CODEPAGE_437 -e NLS_ISO8859_1 -e NLS_UTF8 \
  --set-str FAT_DEFAULT_IOCHARSET iso8859-1 --set-str EXFAT_DEFAULT_IOCHARSET utf8 -d MODULES \
  -e ZERO_CALL_USED_REGS -e SECURITY_DMESG_RESTRICT
$MK O="$OUT/base" olddefconfig >> "$OUT/build.log" 2>&1
cp "$OUT/base/.config" "$OUT/base.config"

# 2. The guest config: the base without the options in trim.list (one per line, # starts a comment), no debug info.
mkdir -p "$OUT/build" && cp "$OUT/base.config" "$OUT/build/.config"
dis=$(grep -v '^#' "$HERE/trim.list" | awk '{print $1}' | grep . | sed 's/^/-d /' | tr '\n' ' ')
$CFG --file "$OUT/build/.config" $dis -d DEBUG_INFO_DWARF5 -e DEBUG_INFO_NONE
$MK O="$OUT/build" olddefconfig >> "$OUT/build.log" 2>&1
# The built-in command line without the parameters whose code is gone, else the kernel hands them to /init.
cmdline="stack_depot_disable=on kasan.stacktrace=off cgroup_disable=pressure ioremap_guard panic=-1 bootconfig arm64.nompam"
grep -q '^CONFIG_CGROUPS=y' "$OUT/build/.config" || cmdline=$(echo "$cmdline" | sed 's/ cgroup_disable=pressure//')
grep -q '^CONFIG_STACKDEPOT=y' "$OUT/build/.config" || cmdline=$(echo "$cmdline" | sed 's/stack_depot_disable=on //')
$CFG --file "$OUT/build/.config" --set-str CMDLINE "$cmdline"
$MK O="$OUT/build" olddefconfig >> "$OUT/build.log" 2>&1
cp "$OUT/build/.config" "$OUT/guest.config"
python3 "$HERE/check-guest-config.py" "$OUT/base.config" "$OUT/guest.config" "$HERE/trim.list" \
  > "$OUT/config-check.txt" || { cat "$OUT/config-check.txt" >&2; echo "the guest config fails its check" >&2; exit 1; }
if cmp -s "$OUT/guest.config" "$HERE/guest.config"; then
  echo "guest.config: same as the published one"
else
  differs "guest.config: differs from the published one (diff $OUT/guest.config $HERE/guest.config)"
fi

# 3. Image.
$MK O="$OUT/build" Image >> "$OUT/build.log" 2>&1
cp "$OUT/build/arch/arm64/boot/Image" "$OUT/Image"
echo "Image: $(sha256sum "$OUT/Image" | cut -d' ' -f1)"
if cmp -s "$OUT/Image" "$HERE/arm64/Image"; then
  echo "Image: byte for byte the published arm64/Image"
else
  differs "Image: differs from the published arm64/Image"
fi
