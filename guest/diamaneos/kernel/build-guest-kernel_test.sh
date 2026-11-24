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
# build-guest-kernel_test.sh: build-guest-kernel.sh with stand-ins for make, clang and the kernel tree. A build that
# gives the published files passes, a checkout at another commit or an output that differs fails, and --new shows the
# differences without failing. Needs bash, git and python3.
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
W=$(mktemp -d "${TMPDIR:-/tmp}/build-guest-kernel-test.XXXXXX")
trap 'rm -rf "$W"' EXIT
fails=0

# The recipe folder: the script, a checker that passes, a trim list, the published files.
mkdir -p "$W/recipe/arm64" "$W/kernel/scripts" "$W/clang/bin" "$W/tools/linux-x86/bin" "$W/tools/common/bison" \
  "$W/bin"
cp "$HERE/build-guest-kernel.sh" "$W/recipe/"
printf '#!/usr/bin/env python3\n' > "$W/recipe/check-guest-config.py"
printf "UNUSED_OPTION\n" > "$W/recipe/trim.list"
printf 'CONFIG_PUBLISHED=y\n' > "$W/recipe/guest.config"
printf 'published image\n' > "$W/recipe/arm64/Image"
for t in clang; do printf '#!/bin/sh\n' > "$W/clang/bin/$t"; done
for t in bison flex m4 gavinhoward-bc; do printf '#!/bin/sh\n' > "$W/tools/linux-x86/bin/$t"; done
# make: the configs and the Image come from $CONFIG and $IMAGE. scripts/config: does nothing.
cat > "$W/bin/make" <<'MAKE'
#!/bin/bash
out=
for a in "$@"; do case $a in O=*) out=${a#O=} ;; esac; done
case "${@: -1}" in
  microdroid_defconfig|olddefconfig) mkdir -p "$out"; cp "$CONFIG" "$out/.config" ;;
  Image) mkdir -p "$out/arch/arm64/boot"; cp "$IMAGE" "$out/arch/arm64/boot/Image" ;;
esac
MAKE
# GNU date -d, as the script uses it: any date reads.
printf '#!/bin/sh\nexit 0\n' > "$W/bin/date"
printf '#!/bin/sh\necho 1\n' > "$W/bin/nproc"
printf '#!/bin/sh\nexit 0\n' > "$W/kernel/scripts/config"
chmod +x "$W"/clang/bin/* "$W"/tools/linux-x86/bin/* "$W"/bin/* "$W/recipe/check-guest-config.py" \
  "$W/kernel/scripts/config"
git -C "$W/kernel" init -q && git -C "$W/kernel" add -A \
  && git -C "$W/kernel" -c user.name=t -c user.email=t@t commit -q -m kernel
commit=$(git -C "$W/kernel" rev-parse HEAD)
pinned() {  # the script with PIN set to $1
  sed "s/^PIN=.*/PIN=$1/" "$HERE/build-guest-kernel.sh" > "$W/recipe/build-guest-kernel.sh"
}
build() {  # expected-exit config image [--new]
  local want=$1 config=$2 image=$3; shift 3
  rm -rf "$W/out"
  CONFIG=$config IMAGE=$image PATH="$W/bin:$PATH" bash "$W/recipe/build-guest-kernel.sh" --kernel "$W/kernel" \
    --clang "$W/clang" --build-tools "$W/tools" --out "$W/out" --jobs 1 "$@" > "$W/log" 2>&1
  local rc=$?
  if { [ "$want" = 0 ] && [ $rc != 0 ]; } || { [ "$want" != 0 ] && [ $rc = 0 ]; }; then
    echo "FAIL: $label (exit $rc)"; sed 's/^/  /' "$W/log"; fails=$((fails + 1))
  else
    echo "ok: $label"
  fi
}
printf 'CONFIG_OTHER=y\n' > "$W/other.config"
printf 'other image\n' > "$W/other.Image"
P=$W/recipe

pinned "$commit"
label="the published files pass"; build 0 "$P/guest.config" "$P/arm64/Image"
label="another Image fails"; build 1 "$P/guest.config" "$W/other.Image"
label="another guest.config fails"; build 1 "$W/other.config" "$P/arm64/Image"
label="--new shows another Image without failing"; build 0 "$P/guest.config" "$W/other.Image" --new
pinned 0000000000000000000000000000000000000000
label="a checkout at another commit fails"; build 1 "$P/guest.config" "$P/arm64/Image"
label="--new builds another commit"; build 0 "$P/guest.config" "$P/arm64/Image" --new

[ $fails = 0 ] && echo "PASS" || { echo "FAILED $fails"; exit 1; }
