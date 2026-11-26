# The DiamaneOS guest kernel

`arm64/Image` is the kernel of the DiamaneOS VMs: the filesystem VM that reads SD cards and USB drives
(`vendor/diamaneos`, fsvm) and the decode service's VM (`packages/apps/DiamaneOSSanitizer`). Both are protected VMs
with a static `/init` in a plain initramfs. The build signs the Image once per VM, together with that VM's initramfs
(`../Android.bp`).

The Image is built from source with the recipe in this folder, and anyone can build it again and compare.

## Source and toolchain

- Kernel: https://github.com/DiamaneOS/kernel_common-6.18, commit
  `2142ff40ddc5a986704586db8b565ae5c14a3933` (release 6.18.64). The guest kernel must have the phone kernel's
  release. The image checks refuse a build where they differ.
- Compiler: the Android tree's clang prebuilt `clang-r596125` (`prebuilts/clang/host/linux-x86/clang-r596125`).
- Build tools: the Android tree's `prebuilts/build-tools` (bison, flex, m4 and bc).
- Host: a Linux x86-64 machine with make, Python 3, Perl and Git.

## What the recipe does

1. `microdroid_defconfig`, plus seccomp, FAT and exFAT, MBR and GPT partitions and their character sets, without
   modules (`base.config`). The guest adds `ZERO_CALL_USED_REGS` and `SECURITY_DMESG_RESTRICT`, both set to `y`
   in this layer. `SCHED_STACK_END_CHECK` stays off and `VMAP_STACK` stays on.
2. The options in `trim.list` taken out: everything neither VM uses, from Microdroid's storage stack to tracing and
   debugfs. Debug info goes too. The built-in command line loses the parameters whose code is gone, and sets
   `vm.compaction_proactiveness` to 0: kcompactd then sleeps until compaction is asked for, so an idle VM keeps
   no timer of its own.
3. `check-guest-config.py` compares the result with the base: only the two named boolean hardening options may
   change from disabled to enabled, no hardening option may become weaker, every option the VMs
   need must be set, and every option of `trim.list` must be gone. Three stay because Kconfig keeps them on arm64
   (`SERIAL_8250_FSL`, `POWER_SUPPLY`, `ARM_GIC_V5`), each with its reason in `trim.list`. A failed check stops the
   build.
4. `Image` with fixed build stamps: the commit's date as the build time, and the user, host and build number the
   published Image carries. The date is given in ISO form, because the kernel also stamps the files of its built-in
   initramfs with it through `date -d`, which does not read Git's default date form. The release takes its suffix,
   `-g` and the commit's first 12 hex digits, from a `localversion` file in the build folder instead of Git, as the
   phone kernel's build does. RANDSTRUCT takes its structure layout seed from that file, so every build of the
   commit lays out structures the same way. With that, two builds from different folders at different times give
   the same bytes.

`guest.config` is the configuration the recipe makes.

## Building it again

```bash
git init kernel
git -C kernel fetch --depth 1 https://github.com/DiamaneOS/kernel_common-6.18 2142ff40ddc5a986704586db8b565ae5c14a3933
git -C kernel checkout FETCH_HEAD
./build-guest-kernel.sh --kernel kernel --aosp /path/to/android-tree --out out
sha256sum out/Image arm64/Image
```

Instead of `--aosp`, `--clang` and `--build-tools` can name the two prebuilts directly. The script fails unless the
checkout is the commit above (`PIN` in the script) and `guest.config` and the Image come out the same as the
published ones. `--new` builds a candidate Image from a changed recipe or commit and only shows the differences.

`check_guest_config_test.py` and `build-guest-kernel_test.sh` test the two scripts.

The published Image is byte for byte what this recipe builds from the commit above with `clang-r596125`.

SHA-256 of `arm64/Image`: `8677cc32983b91453a63e8f68c4208a93947dfe774231ab6000e2fd2da868403`
