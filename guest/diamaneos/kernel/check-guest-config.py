#!/usr/bin/env python3
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
"""check-guest-config.py BASE GUEST TRIM: compares the guest kernel's config (GUEST) with the config it was made from
(BASE, microdroid_defconfig plus the card options, see build-guest-kernel.sh).

Prints the options BASE sets that GUEST does not (or sets differently), the ones GUEST sets that BASE does not, every
hardening option whose value differs without an allowed boolean upgrade, and every option the two VMs need that GUEST lacks (must be
none). Every option TRIM (trim.list) takes out must be gone from GUEST, unless its line says why Kconfig keeps it
("forced: ..."), and such an option must still be set. Exit 1 if a hardening option changed without that upgrade, a needed option is
missing, a trimmed option is still set without its reason, or a reason names an option that is gone."""
import re
import sys


def parse(path):
    d = {}
    for line in open(path):
        m = re.match(r'^CONFIG_([A-Za-z0-9_]+)=(.*)$', line.strip())
        if m:
            d[m.group(1)] = m.group(2)
    return d


# Options that protect. Only the named boolean upgrades below may differ.
HARDENING = re.compile(
    r'^(SECURITY.*|LSM.*|DEFAULT_SECURITY.*|LOCK_DOWN.*|INTEGRITY.*|AUDIT|AUDITSYSCALL|SECCOMP.*|STACKPROTECTOR.*|'
    r'SHADOW_CALL_STACK|CFI.*|RANDOMIZE_.*|RANDOM_KMALLOC_CACHES|VMAP_STACK|STRICT_.*RWX|RODATA_FULL.*|'
    r'ARM64_(PAN|EPAN|SW_TTBR0_PAN|PTR_AUTH.*|BTI.*|MTE|E0PD|POE|GCS|TAGGED_ADDR_ABI)|UNMAP_KERNEL_AT_EL0|'
    r'MITIGATE_.*|CPU_MITIGATIONS|HARDEN_.*|SLAB_FREELIST_.*|SLAB_BUCKETS|SLAB_CANARY|SLAB_MERGE_DEFAULT|SLUB_DEBUG|'
    r'SHUFFLE_PAGE_ALLOCATOR|INIT_STACK.*|INIT_ON_.*|PAGE_SANITIZE.*|FORTIFY_SOURCE|HARDENED_USERCOPY.*|'
    r'LIST_HARDENED|BUG_ON_DATA_CORRUPTION|DEBUG_(SG|LIST|CREDENTIALS|NOTIFIERS|VIRTUAL|WX|KERNEL|MISC)|UBSAN.*|'
    r'KASAN.*|KFENCE.*|PANIC_ON_OOPS|PANIC_TIMEOUT|STATIC_USERMODEHELPER.*|DEFAULT_MMAP_MIN_ADDR|ARCH_MMAP_RND.*|'
    r'IOMMU_DEFAULT_.*|IOMMU_DMA|PKVM_PVIOMMU|ARM_PKVM_GUEST|DMA_RESTRICTED_POOL|GUNYAH.*|MODULES|MODULE_SIG.*|'
    r'DEVMEM|DEVPORT|STRICT_DEVMEM|IO_STRICT_DEVMEM|PROC_MEM_.*|LEGACY_TIOCSTI|LDISC_AUTOLOAD|LEGACY_PTYS|COMPAT|'
    r'ZERO_CALL_USED_REGS|SCHED_STACK_END_CHECK|KSTACK_ERASE.*|GCC_PLUGIN.*|PAGE_TABLE_CHECK.*|HW_RANDOM_ARM_SMCCC_TRNG|VMGENID|'
    r'OPEN_DICE|BLK_DEV_WRITE_MOUNTED|SYN_COOKIES|NETWORK_SECMARK|LSM_MMAP_MIN_ADDR|RELOCATABLE|FRAME_WARN)$')

# These boolean options may become y when Microdroid leaves them off, never the reverse.
STRICTER_ALLOWED = frozenset(('ZERO_CALL_USED_REGS', 'SECURITY_DMESG_RESTRICT'))

# What the two VMs need (y in NEW).
NEEDED = ('SECCOMP SECCOMP_FILTER FAT_FS VFAT_FS EXFAT_FS MSDOS_PARTITION EFI_PARTITION PARTITION_ADVANCED NLS '
          'NLS_CODEPAGE_437 NLS_ISO8859_1 NLS_UTF8 BLOCK BLK_DEV VIRTIO_BLK VSOCKETS VIRTIO_VSOCKETS DEVTMPFS PROC_FS '
          'PROC_SYSCTL PROC_PAGE_MONITOR SYSFS TMPFS SHMEM MEMFD_CREATE BLK_DEV_INITRD VIRTIO_CONSOLE HVC_DRIVER '
          'VIRTIO_PCI PCI PCI_HOST_GENERIC VIRTIO UNIX NET MULTIUSER FUTEX EPOLL POSIX_TIMERS ADVISE_SYSCALLS '
          'BINFMT_ELF SMP ARM64_BTI ARM64_SVE ARM64_SME GUNYAH_GUEST ARM_PKVM_GUEST DMA_RESTRICTED_POOL SWIOTLB '
          'SERIAL_8250 SERIAL_8250_CONSOLE SERIAL_OF_PLATFORM NULL_TTY BOOT_CONFIG RTC_CLASS RTC_HCTOSYS '
          'RTC_DRV_PL030 RTC_DRV_PL031 HW_RANDOM HW_RANDOM_ARM_SMCCC_TRNG VIRTIO_BALLOON PAGE_REPORTING COMPACTION '
          'MIGRATION LRU_GEN CPU_FREQ CPUFREQ_VIRT ARM_SCMI_PERF_DOMAIN VCPU_STALL_DETECTOR OPEN_DICE ARM_GIC_V3 '
          'ARM_ARCH_TIMER ARM_PSCI_FW DEBUG_STACK_USAGE PRINTK KALLSYMS SECURITY_SELINUX AUDIT').split()

def parse_trim(path):
    """trim.list: one option per line, # starts a comment, "forced: why" after the name for one Kconfig keeps."""
    trimmed, forced = [], set()
    for line in open(path):
        fields = line.split('#', 1)[0].split(None, 1) if not line.startswith('#') else []
        if not fields:
            continue
        if fields[0] in trimmed:
            sys.exit('trim.list: duplicate option: %s' % fields[0])
        trimmed.append(fields[0])
        if len(fields) > 1:
            if not fields[1].startswith('forced:'):
                sys.exit('trim.list: %s: after the name only "forced: why"' % fields[0])
            if not fields[1][len('forced:'):].strip():
                sys.exit('trim.list: %s: empty forced explanation' % fields[0])
            forced.add(fields[0])
    return trimmed, forced


if len(sys.argv) != 4:
    sys.exit('usage: check-guest-config.py BASE GUEST TRIM')
base, new = parse(sys.argv[1]), parse(sys.argv[2])
trimmed, forced = parse_trim(sys.argv[3])
gone = sorted(k for k in base if k not in new)
changed = sorted(k for k in base if k in new and base[k] != new[k])
added = sorted(k for k in new if k not in base)
print('options set: base %d, new %d' % (len(base), len(new)))
print('REMOVED %d: %s' % (len(gone), ' '.join(gone)))
print('CHANGED %d: %s' % (len(changed), ' '.join('%s %s->%s' % (k, base[k], new[k]) for k in changed)))
print('ADDED %d: %s' % (len(added), ' '.join('%s=%s' % (k, new[k]) for k in added)))
hard = sorted(set(k for k in list(base) + list(new) if HARDENING.match(k)))
upgraded = [k for k in hard if k in STRICTER_ALLOWED and base.get(k, 'n') == 'n' and new.get(k) == 'y']
hdiff = [k for k in hard if base.get(k) != new.get(k) and k not in upgraded]
print('HARDENING stricter %d: %s' % (len(upgraded), ' '.join(upgraded)))
print('HARDENING options compared %d, differing %d: %s' % (len(hard), len(hdiff),
      ' '.join('%s %s->%s' % (k, base.get(k), new.get(k)) for k in hdiff)))
print('HARDENING kept: %s' % ' '.join('%s=%s' % (k, new[k]) for k in hard if k in new))
missing = [k for k in NEEDED if new.get(k) != 'y']
print('NEEDED %d, missing %d: %s' % (len(NEEDED), len(missing), ' '.join(missing)))
kept = [k for k in trimmed if new.get(k, 'n') != 'n']
unexplained = [k for k in kept if k not in forced]
stale = sorted(k for k in forced if k not in kept)
print('TRIMMED %d, kept by Kconfig %d: %s' % (len(trimmed), len(kept) - len(unexplained),
      ' '.join(k for k in kept if k in forced)))
print('TRIMMED but still set, no reason given %d: %s' % (len(unexplained), ' '.join(unexplained)))
print('FORCED but gone %d: %s' % (len(stale), ' '.join(stale)))
sys.exit(1 if hdiff or missing or unexplained or stale else 0)
