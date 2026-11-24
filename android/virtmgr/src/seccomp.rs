// Copyright (C) 2026 DiamaneOS
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! DiamaneOS: the VM manager's seccomp filter. virtmgr serves its client, the guest's
//! VirtualMachineService over vsock and every VM's crosvm, and starts a crosvm for each VM for as
//! long as it lives, so there is no point after its last exec. It installs this filter in main
//! after its setup and before it tells the client it is ready, on every thread (TSYNC). From then
//! on everything it does runs under the filter.
//!
//! crosvm inherits the filter (filters stack and survive exec), so the list is virtmgr's own calls
//! plus crosvm's whole life: its start, its setup and everything on crosvm's own list. crosvm's own
//! filter (diamaneos_seccomp.rs in crosvm) narrows crosvm further once its setup is done. What this
//! filter refuses is what neither needs: ptrace, process_vm_*, bpf, perf_event_open, mount,
//! unshare, setns, keyctl, io_uring, userfaultfd, module loading, sockets other than AF_UNIX and
//! AF_VSOCK, ioctls outside `IOCTLS`, signals to process groups or everyone.
//!
//! crosvm's start before its own filter needs a few calls only with exact arguments: mlock2 with
//! MLOCK_ONFAULT (--lock-guest-memory), seccomp to put on its own filter with TSYNC (a filter can
//! only narrow what a process may do), TCSETS on descriptor 0 (raw mode when stdin is a terminal)
//! and getrlimit of RLIMIT_NOFILE (bionic's fdsan sizes its descriptor table once, on whichever
//! thread first uses a descriptor above 127).
//!
//! no_new_privs keeps crosvm's CAP_SYS_NICE: under no_new_privs the kernel limits the capabilities
//! a file grants to what the parent holds, and virtmgr holds CAP_SYS_NICE itself. It does block the
//! SELinux domain change to crosvm unless process2 nnp_transition is allowed, which
//! vendor_diamaneos allows from each VM manager domain to the crosvm domains
//! (sepolicy/system_ext/private/vm_filters.te). It grants no new domain change.
//!
//! Anything else kills the process on user builds (KILL_ON_USER_BUILDS): the client sees its VM
//! manager end, a crosvm its VM end. Debuggable builds log it (audit type 1326) and allow it, so
//! that a phone run of every VM use shows what is missing, unless the test hook (TEST_ENV) asks
//! for the user-build action. The user-build action is also text in the program (USER_BUILDS),
//! which the start line names and the image check of diamaneos-tools reads from the built virtmgr.
//!
//! Where the list comes from: the code after the install point (the RPC services, crosvm.rs, the
//! payload and composite disk code, vm_pool.rs, the PSI monitor, tombstoned and statsd), the start
//! of crosvm (fork, exec, the dynamic linker, bionic's start), crosvm's setup and its own list, and
//! an strace of every VM use on the phone.

#![allow(dead_code)]

use anyhow::{ensure, Result};
use log::warn;

/// One BPF instruction (struct sock_filter).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Insn {
    code: u16,
    jt: u8,
    jf: u8,
    k: u32,
}

const BPF_LD_W_ABS: u16 = 0x20;
const BPF_JEQ_K: u16 = 0x15;
const BPF_JSET_K: u16 = 0x45;
const BPF_RET_K: u16 = 0x06;

pub const RET_KILL_PROCESS: u32 = 0x8000_0000;
pub const RET_ERRNO: u32 = 0x0005_0000;
pub const RET_LOG: u32 = 0x7ffc_0000;
pub const RET_ALLOW: u32 = 0x7fff_0000;

/// Offsets in struct seccomp_data.
const NR: u32 = 0;
const ARCH: u32 = 4;
const fn arg_lo(i: u32) -> u32 {
    16 + 8 * i
}

#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH: u32 = 0xC000_00B7; // AUDIT_ARCH_AARCH64
#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xC000_003E; // AUDIT_ARCH_X86_64

/// Newer calls, numbered alike on every architecture since Linux 5.1.
const SYS_PIDFD_OPEN: libc::c_long = 434;
const SYS_CLONE3: libc::c_long = 435;
const SYS_PIDFD_GETFD: libc::c_long = 438;

/// The system calls virtmgr and the crosvm it starts make after the filter.
pub const ALLOWED: &[(libc::c_long, &str)] = &[
    // Memory: bionic, scudo, Rust's std, the dynamic linker in a new crosvm, guest memory (locked
    // and kept from forks).
    (libc::SYS_brk, "brk"),
    (libc::SYS_mmap, "mmap"),
    (libc::SYS_munmap, "munmap"),
    (libc::SYS_mremap, "mremap"),
    (libc::SYS_mprotect, "mprotect"),
    (libc::SYS_madvise, "madvise"),
    (libc::SYS_mlock, "mlock"),
    (libc::SYS_munlock, "munlock"),
    (libc::SYS_prctl, "prctl"),
    (libc::SYS_getrandom, "getrandom"),
    (libc::SYS_getcpu, "getcpu"),
    // Threads and their scheduling: RPC binder's and the VM monitors' threads, crosvm's vCPU setup
    // (affinity, real time limit, uclamp).
    (libc::SYS_futex, "futex"),
    (libc::SYS_set_robust_list, "set_robust_list"),
    (libc::SYS_set_tid_address, "set_tid_address"),
    (libc::SYS_sched_yield, "sched_yield"),
    (libc::SYS_sched_getaffinity, "sched_getaffinity"),
    (libc::SYS_sched_setaffinity, "sched_setaffinity"),
    (libc::SYS_sched_getattr, "sched_getattr"),
    (libc::SYS_sched_setattr, "sched_setattr"),
    (libc::SYS_sched_getscheduler, "sched_getscheduler"),
    (libc::SYS_sched_setscheduler, "sched_setscheduler"),
    (libc::SYS_getpriority, "getpriority"),
    (libc::SYS_setpriority, "setpriority"),
    (libc::SYS_gettid, "gettid"),
    (libc::SYS_getpid, "getpid"),
    (libc::SYS_getppid, "getppid"),
    (libc::SYS_getuid, "getuid"),
    (libc::SYS_geteuid, "geteuid"),
    (libc::SYS_getgid, "getgid"),
    (libc::SYS_getegid, "getegid"),
    (libc::SYS_capget, "capget"),
    (libc::SYS_prlimit64, "prlimit64"),
    (libc::SYS_uname, "uname"),
    (libc::SYS_exit, "exit"),
    (libc::SYS_exit_group, "exit_group"),
    (libc::SYS_restart_syscall, "restart_syscall"),
    // A crosvm per VM: fork, exec, wait. clone and the signals are in `build`.
    (libc::SYS_execve, "execve"),
    (libc::SYS_wait4, "wait4"),
    (libc::SYS_waitid, "waitid"),
    // Signals: handlers, masks, crosvm's vCPU kick signal cleared again, crosvm's SIGCHLD
    // descriptor.
    (libc::SYS_rt_sigaction, "rt_sigaction"),
    (libc::SYS_rt_sigprocmask, "rt_sigprocmask"),
    (libc::SYS_rt_sigreturn, "rt_sigreturn"),
    (libc::SYS_rt_sigtimedwait, "rt_sigtimedwait"),
    (libc::SYS_rt_sigpending, "rt_sigpending"),
    (libc::SYS_sigaltstack, "sigaltstack"),
    (libc::SYS_signalfd4, "signalfd4"),
    // Time and timers.
    (libc::SYS_clock_gettime, "clock_gettime"),
    (libc::SYS_clock_getres, "clock_getres"),
    (libc::SYS_clock_nanosleep, "clock_nanosleep"),
    (libc::SYS_nanosleep, "nanosleep"),
    (libc::SYS_gettimeofday, "gettimeofday"),
    (libc::SYS_timerfd_create, "timerfd_create"),
    (libc::SYS_timerfd_settime, "timerfd_settime"),
    (libc::SYS_timerfd_gettime, "timerfd_gettime"),
    // Files: the VM's images and disks, its temporary folder (composite disk headers, the crosvm
    // control socket), the instance disk (memfd), the pool and Gunyah devices, /proc for crosvm's
    // arguments and the PSI monitor.
    (libc::SYS_openat, "openat"),
    (libc::SYS_close, "close"),
    (libc::SYS_read, "read"),
    (libc::SYS_write, "write"),
    (libc::SYS_readv, "readv"),
    (libc::SYS_writev, "writev"),
    (libc::SYS_pread64, "pread64"),
    (libc::SYS_pwrite64, "pwrite64"),
    (libc::SYS_preadv, "preadv"),
    (libc::SYS_pwritev, "pwritev"),
    (libc::SYS_lseek, "lseek"),
    (libc::SYS_dup, "dup"),
    (libc::SYS_dup3, "dup3"),
    (libc::SYS_fcntl, "fcntl"),
    #[cfg(target_arch = "aarch64")]
    (79, "newfstatat"),
    #[cfg(target_arch = "x86_64")]
    (libc::SYS_newfstatat, "newfstatat"),
    #[cfg(target_arch = "aarch64")]
    (80, "fstat"),
    #[cfg(target_arch = "x86_64")]
    (libc::SYS_fstat, "fstat"),
    (libc::SYS_statx, "statx"),
    #[cfg(target_arch = "aarch64")]
    (44, "fstatfs"),
    #[cfg(target_arch = "x86_64")]
    (libc::SYS_fstatfs, "fstatfs"),
    (libc::SYS_faccessat, "faccessat"),
    (libc::SYS_readlinkat, "readlinkat"),
    (libc::SYS_getdents64, "getdents64"),
    (libc::SYS_mkdirat, "mkdirat"),
    (libc::SYS_unlinkat, "unlinkat"),
    (libc::SYS_fgetxattr, "fgetxattr"),
    (libc::SYS_getxattr, "getxattr"),
    (libc::SYS_lgetxattr, "lgetxattr"),
    (libc::SYS_fsync, "fsync"),
    (libc::SYS_fdatasync, "fdatasync"),
    (libc::SYS_fallocate, "fallocate"),
    #[cfg(target_arch = "aarch64")]
    (46, "ftruncate"),
    #[cfg(target_arch = "x86_64")]
    (libc::SYS_ftruncate, "ftruncate"),
    (libc::SYS_memfd_create, "memfd_create"),
    (libc::SYS_pipe2, "pipe2"),
    (libc::SYS_eventfd2, "eventfd2"),
    (libc::SYS_ppoll, "ppoll"),
    (libc::SYS_pselect6, "pselect6"),
    (libc::SYS_epoll_create1, "epoll_create1"),
    (libc::SYS_epoll_ctl, "epoll_ctl"),
    (libc::SYS_epoll_pwait, "epoll_pwait"),
    // Sockets: RPC binder to the client and over vsock to the guest, the crosvm control socket,
    // tombstoned, logd. socket itself is in `build`.
    (libc::SYS_bind, "bind"),
    (libc::SYS_listen, "listen"),
    (libc::SYS_accept4, "accept4"),
    (libc::SYS_connect, "connect"),
    (libc::SYS_socketpair, "socketpair"),
    (libc::SYS_sendmsg, "sendmsg"),
    (libc::SYS_recvmsg, "recvmsg"),
    (libc::SYS_sendto, "sendto"),
    (libc::SYS_recvfrom, "recvfrom"),
    (libc::SYS_setsockopt, "setsockopt"),
    (libc::SYS_getsockopt, "getsockopt"),
    (libc::SYS_getsockname, "getsockname"),
    (libc::SYS_getpeername, "getpeername"),
    (libc::SYS_shutdown, "shutdown"),
    #[cfg(target_arch = "aarch64")]
    (293, "rseq"),
    #[cfg(target_arch = "x86_64")]
    (334, "rseq"),
    #[cfg(target_arch = "x86_64")]
    (libc::SYS_poll, "poll"),
    #[cfg(target_arch = "x86_64")]
    (libc::SYS_arch_prctl, "arch_prctl"),
];

/// ioctl request encoding (asm-generic/ioctl.h).
const fn io(ty: u32, nr: u32) -> u32 {
    (ty << 8) | nr
}
const fn iow(ty: u32, nr: u32, size: u32) -> u32 {
    (1 << 30) | (size << 16) | (ty << 8) | nr
}
const fn ior(ty: u32, nr: u32, size: u32) -> u32 {
    (2 << 30) | (size << 16) | (ty << 8) | nr
}
const fn iowr(ty: u32, nr: u32, size: u32) -> u32 {
    (3 << 30) | (size << 16) | (ty << 8) | nr
}

/// Gunyah's ioctl types (crosvm hypervisor/src/gunyah/gunyah_sys.rs), vhost's, binder's and the
/// block layer's.
const GH: u32 = 0x47;
const GH_ANDROID: u32 = 0x41;
const VHOST: u32 = 0xAF;
const BINDER: u32 = 0x62;
const BLOCK: u32 = 0x12;

/// ioctl commands virtmgr and crosvm use.
pub const IOCTLS: &[(u32, &str)] = &[
    // Binder: VirtualizationServiceInternal, the secretkeeper and VM capabilities HALs, statsd.
    (iowr(BINDER, 1, 48), "BINDER_WRITE_READ"),
    (iow(BINDER, 5, 4), "BINDER_SET_MAX_THREADS"),
    (iow(BINDER, 8, 4), "BINDER_THREAD_EXIT"),
    (iowr(BINDER, 9, 4), "BINDER_VERSION"),
    (iow(BINDER, 16, 4), "BINDER_ENABLE_ONEWAY_SPAM_DETECTION"),
    (iowr(BINDER, 17, 12), "BINDER_GET_EXTENDED_ERROR"),
    // The VM memory pool (vm_pool.rs).
    (iow(GH_ANDROID, 0x30, 16), "GH_ANDROID_CREATE_POOL_MEM_FD"),
    // Gunyah, crosvm's setup and run (crosvm hypervisor/src/gunyah).
    (io(GH, 0x0), "GH_CREATE_VM"),
    (iow(GH, 0x1, 32), "GH_VM_SET_USER_MEM_REGION"),
    (iow(GH, 0x2, 16), "GH_VM_SET_DTB_CONFIG"),
    (io(GH, 0x3), "GH_VM_START"),
    (iow(GH, 0x4, 16), "GH_VM_ADD_FUNCTION"),
    (io(GH, 0x5), "GH_VCPU_RUN"),
    (io(GH, 0x6), "GH_VCPU_MMAP_SIZE"),
    (iow(GH, 0x7, 16), "GH_VM_REMOVE_FUNCTION"),
    (iow(GH, 0xa, 16), "GH_VM_SET_BOOT_CONTEXT"),
    (iow(GH_ANDROID, 0x11, 32), "GH_VM_ANDROID_LEND_USER_MEM"),
    (iow(GH_ANDROID, 0x12, 16), "GH_VM_ANDROID_SET_FW_CONFIG"),
    (iow(GH_ANDROID, 0x13, 16), "GH_VM_RECLAIM_REGION"),
    (iow(GH_ANDROID, 0x15, 40), "GH_VM_ANDROID_MAP_CMA_MEM"),
    (iow(GH_ANDROID, 0x16, 16), "GH_VM_ANDROID_SET_AUTH_TYPE"),
    // vhost-vsock, crosvm's setup, activation and reset.
    (ior(VHOST, 0x00, 8), "VHOST_GET_FEATURES"),
    (iow(VHOST, 0x00, 8), "VHOST_SET_FEATURES"),
    (io(VHOST, 0x01), "VHOST_SET_OWNER"),
    (io(VHOST, 0x02), "VHOST_RESET_OWNER"),
    (iow(VHOST, 0x03, 8), "VHOST_SET_MEM_TABLE"),
    (iow(VHOST, 0x10, 8), "VHOST_SET_VRING_NUM"),
    (iow(VHOST, 0x11, 40), "VHOST_SET_VRING_ADDR"),
    (iow(VHOST, 0x12, 8), "VHOST_SET_VRING_BASE"),
    (iowr(VHOST, 0x12, 8), "VHOST_GET_VRING_BASE"),
    (iow(VHOST, 0x20, 8), "VHOST_SET_VRING_KICK"),
    (iow(VHOST, 0x21, 8), "VHOST_SET_VRING_CALL"),
    (iow(VHOST, 0x22, 8), "VHOST_SET_VRING_ERR"),
    (iow(VHOST, 0x60, 8), "VHOST_VSOCK_SET_GUEST_CID"),
    (iow(VHOST, 0x61, 4), "VHOST_VSOCK_SET_RUNNING"),
    // Block devices: the card's sector size (crosvm.rs) and size (composite.rs), crosvm's discard
    // on a raw card.
    (io(BLOCK, 104), "BLKSSZGET"),
    (ior(BLOCK, 114, 8), "BLKGETSIZE64"),
    (io(BLOCK, 119), "BLKDISCARD"),
    // Terminals (whether a console descriptor is one), descriptor flags, the bytes waiting on a
    // socket.
    (0x5401, "TCGETS"),
    (0x5421, "FIONBIO"),
    (0x541B, "FIONREAD"),
    (0x5451, "FIOCLEX"),
    (0x5450, "FIONCLEX"),
];

/// TCSETS: crosvm puts a terminal stdin into raw mode before its own filter.
const TCSETS: u32 = 0x5402;
/// RLIMIT_NOFILE and MLOCK_ONFAULT, alike on every architecture (libc's types differ between
/// bionic and glibc, the test compares them).
const RLIMIT_NOFILE: u32 = 7;
const MLOCK_ONFAULT: u32 = 1;

/// virtmgr's filter. `default` is what happens to any other call: RET_KILL_PROCESS on user builds
/// (KILL_ON_USER_BUILDS), RET_LOG otherwise. It does not name virtmgr's pid: every crosvm it
/// starts runs the same program with its own pid.
pub fn build(default: u32) -> Vec<Insn> {
    let ld = |k| Insn { code: BPF_LD_W_ABS, jt: 0, jf: 0, k };
    let ret = |k| Insn { code: BPF_RET_K, jt: 0, jf: 0, k };
    let jeq = |k, jt, jf| Insn { code: BPF_JEQ_K, jt, jf, k };
    let jset = |k, jt, jf| Insn { code: BPF_JSET_K, jt, jf, k };
    let deny = RET_ERRNO | libc::EPERM as u32;
    let mut p = vec![ld(ARCH), jeq(AUDIT_ARCH, 1, 0), ret(RET_KILL_PROCESS), ld(NR)];
    // Signals with a pid in the first argument: one process (virtmgr's crosvm children, by pid, its
    // own threads, crosvm's own threads), never 0 or a negative pid (a process group, or everyone).
    // SELinux and the user id bound which process that can be.
    for nr in
        [libc::SYS_kill, libc::SYS_tgkill, libc::SYS_rt_sigqueueinfo, libc::SYS_rt_tgsigqueueinfo]
    {
        p.extend([
            jeq(nr as u32, 0, 5),
            ld(arg_lo(0)),
            jeq(0, 2, 0),
            jset(0x8000_0000, 1, 0),
            ret(RET_ALLOW),
            ret(deny),
        ]);
    }
    for nr in [libc::SYS_tkill, SYS_PIDFD_OPEN, SYS_PIDFD_GETFD] {
        p.extend([jeq(nr as u32, 0, 1), ret(deny)]);
    }
    // fork for crosvm and threads. clone3 passes its flags in memory, which a filter cannot read:
    // ENOSYS, so libc uses clone.
    p.extend([jeq(libc::SYS_clone as u32, 0, 1), ret(RET_ALLOW)]);
    p.extend([jeq(SYS_CLONE3 as u32, 0, 1), ret(RET_ERRNO | libc::ENOSYS as u32)]);
    // fdsan's descriptor limit, nothing else.
    p.extend([
        jeq(libc::SYS_getrlimit as u32, 0, 4),
        ld(arg_lo(0)),
        jeq(RLIMIT_NOFILE, 0, 1),
        ret(RET_ALLOW),
        ret(default),
    ]);
    // crosvm locks its guest memory as it is touched.
    p.extend([
        jeq(libc::SYS_mlock2 as u32, 0, 4),
        ld(arg_lo(2)),
        jeq(MLOCK_ONFAULT, 0, 1),
        ret(RET_ALLOW),
        ret(default),
    ]);
    // crosvm puts on its own filter, on all its threads.
    p.extend([
        jeq(libc::SYS_seccomp as u32, 0, 6),
        ld(arg_lo(0)),
        jeq(SECCOMP_SET_MODE_FILTER, 0, 3),
        ld(arg_lo(1)),
        jeq(SECCOMP_FILTER_FLAG_TSYNC, 0, 1),
        ret(RET_ALLOW),
        ret(default),
    ]);
    // Unix and vsock sockets only.
    p.extend([
        jeq(libc::SYS_socket as u32, 0, 5),
        ld(arg_lo(0)),
        jeq(libc::AF_UNIX as u32, 1, 0),
        jeq(libc::AF_VSOCK as u32, 0, 1),
        ret(RET_ALLOW),
        ret(default),
    ]);
    // TCSETS on stdin only. Any other ioctl goes on to the list below.
    p.extend([
        jeq(libc::SYS_ioctl as u32, 0, 6),
        ld(arg_lo(0)),
        jeq(0, 0, 3),
        ld(arg_lo(1)),
        jeq(TCSETS, 0, 1),
        ret(RET_ALLOW),
        ld(NR),
    ]);
    // ioctl: the commands in IOCTLS.
    p.extend([jeq(libc::SYS_ioctl as u32, 0, (IOCTLS.len() + 3) as u8), ld(arg_lo(1))]);
    for (i, (cmd, _)) in IOCTLS.iter().enumerate() {
        p.push(jeq(*cmd, (IOCTLS.len() - i) as u8, 0));
    }
    p.extend([ret(default), ret(RET_ALLOW)]);
    for (nr, _) in ALLOWED {
        p.extend([jeq(*nr as u32, 0, 1), ret(RET_ALLOW)]);
    }
    p.push(ret(default));
    p
}

#[repr(C)]
struct Prog {
    len: u16,
    filter: *const Insn,
}

const SECCOMP_SET_MODE_FILTER: libc::c_uint = 1;
const SECCOMP_FILTER_FLAG_TSYNC: libc::c_uint = 1;

/// Installs `filter` on every thread of this process.
fn install_tsync(filter: &[Insn]) -> Result<()> {
    ensure!(filter.len() < u16::MAX as usize, "filter too long");
    // SAFETY: prctl with these arguments only sets no_new_privs for this process.
    let r = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    ensure!(r == 0, "no_new_privs: {}", std::io::Error::last_os_error());
    let prog = Prog { len: filter.len() as u16, filter: filter.as_ptr() };
    // SAFETY: seccomp reads the program, which stays valid for the call. The kernel copies it.
    let r = unsafe {
        libc::syscall(
            libc::SYS_seccomp,
            SECCOMP_SET_MODE_FILTER,
            SECCOMP_FILTER_FLAG_TSYNC,
            &prog as *const Prog,
        )
    };
    ensure!(r >= 0, "seccomp: {}", std::io::Error::last_os_error());
    // With TSYNC a positive result is the id of a thread that could not take the filter, and then
    // no thread has it.
    ensure!(r == 0, "seccomp: thread {r} could not take the filter");
    Ok(())
}

/// Whether user builds kill on a call outside the list: true since the phone gate ran every VM use
/// with zero audit type 1326 lines from virtmgr or its crosvm.
pub const KILL_ON_USER_BUILDS: bool = true;

/// The user-build action as text in the program: the start line names it, and the image check of
/// diamaneos-tools (vm-filters-kill-on-user-builds) reads it from the built virtmgr. `action` reads
/// it too, so it stays in the program.
pub const USER_BUILDS: &str = if KILL_ON_USER_BUILDS {
    "DiamaneOS seccomp filter: user builds kill"
} else {
    "DiamaneOS seccomp filter: user builds log"
};

/// The test hook of debuggable builds, read from virtmgr's environment, which it has from its
/// client (`vm run-microdroid` in an adb shell) and hands on to each crosvm: "user" gives the
/// filter the user-build action. "probe-virtmgr" does too, and right after the filter is on
/// virtmgr makes one call that no VM filter lists (PROBE_CALL), so it ends by SIGSYS where user
/// builds kill. Any other value, or none, changes nothing here. User builds never read it.
pub const TEST_ENV: &str = "DIAMANEOS_VM_FILTER_TEST";

/// The probe's call: getpgid, on no list of the VM filters, harmless where it is only logged.
pub const PROBE_CALL: libc::c_long = libc::SYS_getpgid;

/// What the filter does with a call outside the list (`default` of `build`), and whether virtmgr
/// makes one on purpose. `test` is TEST_ENV's value on a debuggable build.
pub fn action(debuggable: bool, test: Option<&str>) -> (u32, bool) {
    let user = if std::hint::black_box(USER_BUILDS).ends_with(" kill") {
        RET_KILL_PROCESS
    } else {
        RET_LOG
    };
    if !debuggable {
        return (user, false);
    }
    match test {
        Some("user") => (user, false),
        Some("probe-virtmgr") => (user, true),
        _ => (RET_LOG, false),
    }
}

/// The probe: one call outside the list, on purpose.
fn probe() {
    warn!("seccomp filter test: a call outside the list (getpgid) on purpose");
    // SAFETY: getpgid(0) only reads this process's group.
    unsafe { libc::syscall(PROBE_CALL, 0) };
    warn!("seccomp filter test: still running after the call outside the list");
}

/// Whether this is a debuggable build (ro.debuggable=1): the filter then logs instead of killing,
/// unless the test hook asks otherwise.
#[cfg(target_os = "android")]
fn debuggable() -> bool {
    const PROP_VALUE_MAX: usize = 92;
    let mut v = [0u8; PROP_VALUE_MAX];
    // SAFETY: __system_property_get writes at most PROP_VALUE_MAX bytes into `v`, NUL-ended.
    let n =
        unsafe { libc::__system_property_get(c"ro.debuggable".as_ptr(), v.as_mut_ptr().cast()) };
    n == 1 && v[0] == b'1'
}

#[cfg(not(target_os = "android"))]
fn debuggable() -> bool {
    true
}

/// virtmgr's filter on every thread: kill on user builds, log on debuggable builds.
pub fn install() -> Result<&'static str> {
    let debuggable = debuggable();
    let test = if debuggable { std::env::var(TEST_ENV).ok() } else { None };
    let (default, probe_now) = action(debuggable, test.as_deref());
    install_tsync(&build(default))?;
    if probe_now {
        probe();
    }
    Ok(if default == RET_KILL_PROCESS { "killing" } else { "logging" })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the program on one call, as the kernel would: the action it returns.
    fn eval(p: &[Insn], nr: libc::c_long, args: [u64; 6]) -> u32 {
        let field = |k: u32| -> u32 {
            match k {
                NR => nr as u32,
                ARCH => AUDIT_ARCH,
                k if (16..64).contains(&k) && k % 8 == 0 => args[((k - 16) / 8) as usize] as u32,
                k => panic!("load at {k}"),
            }
        };
        let (mut pc, mut a) = (0usize, 0u32);
        loop {
            let i = p[pc];
            pc += 1;
            match i.code {
                BPF_LD_W_ABS => a = field(i.k),
                BPF_JEQ_K => pc += if a == i.k { i.jt } else { i.jf } as usize,
                BPF_JSET_K => pc += if a & i.k != 0 { i.jt } else { i.jf } as usize,
                BPF_RET_K => return i.k,
                c => panic!("code {c:#x}"),
            }
        }
    }

    #[test]
    fn ioctl_numbers() {
        // From the kernel headers, crosvm's bindings and vm_pool.rs, as strace prints them.
        let n = |s: &str| IOCTLS.iter().find(|(_, x)| *x == s).unwrap().0;
        assert_eq!(n("BINDER_WRITE_READ"), 0xc030_6201);
        assert_eq!(n("BINDER_VERSION"), 0xc004_6209);
        assert_eq!(n("GH_ANDROID_CREATE_POOL_MEM_FD"), 0x4010_4130);
        assert_eq!(n("GH_VM_SET_USER_MEM_REGION"), 0x4020_4701);
        assert_eq!(n("GH_VM_ANDROID_MAP_CMA_MEM"), 0x4028_4115);
        assert_eq!(n("VHOST_SET_VRING_ADDR"), 0x4028_af11);
        assert_eq!(n("BLKSSZGET"), 0x1268);
        assert_eq!(n("BLKGETSIZE64"), 0x8008_1272);
        assert_eq!(n("BLKDISCARD"), 0x1277);
    }

    #[test]
    fn what_the_filter_decides() {
        let p = build(RET_KILL_PROCESS);
        let eperm = RET_ERRNO | libc::EPERM as u32;
        let call = |nr, args| eval(&p, nr, args);
        // Signals: to one process, never to a group or everyone.
        assert_eq!(call(libc::SYS_kill, [4321, 9, 0, 0, 0, 0]), RET_ALLOW);
        assert_eq!(call(libc::SYS_tgkill, [4321, 4322, 34, 0, 0, 0]), RET_ALLOW);
        assert_eq!(call(libc::SYS_kill, [0, 9, 0, 0, 0, 0]), eperm);
        assert_eq!(call(libc::SYS_kill, [u32::MAX as u64, 9, 0, 0, 0, 0]), eperm); // -1
        assert_eq!(call(libc::SYS_kill, [(-4321i32) as u32 as u64, 9, 0, 0, 0, 0]), eperm);
        assert_eq!(call(libc::SYS_tkill, [4321, 9, 0, 0, 0, 0]), eperm);
        assert_eq!(call(SYS_PIDFD_OPEN, [1, 0, 0, 0, 0, 0]), eperm);
        // A crosvm per VM: fork and exec.
        assert_eq!(call(libc::SYS_clone, [libc::SIGCHLD as u64, 0, 0, 0, 0, 0]), RET_ALLOW);
        assert_eq!(call(SYS_CLONE3, [0; 6]), RET_ERRNO | libc::ENOSYS as u32);
        assert_eq!(call(libc::SYS_execve, [0; 6]), RET_ALLOW);
        assert_eq!(call(libc::SYS_execveat, [0; 6]), RET_KILL_PROCESS);
        // Unix and vsock sockets only.
        assert_eq!(call(libc::SYS_socket, [libc::AF_UNIX as u64, 5, 0, 0, 0, 0]), RET_ALLOW);
        assert_eq!(call(libc::SYS_socket, [libc::AF_VSOCK as u64, 1, 0, 0, 0, 0]), RET_ALLOW);
        assert_eq!(call(libc::SYS_socket, [libc::AF_INET as u64, 2, 0, 0, 0, 0]), RET_KILL_PROCESS);
        assert_eq!(
            call(libc::SYS_socket, [libc::AF_NETLINK as u64, 3, 0, 0, 0, 0]),
            RET_KILL_PROCESS
        );
        // The listed ioctls only.
        for (cmd, name) in IOCTLS {
            assert_eq!(call(libc::SYS_ioctl, [3, *cmd as u64, 0, 0, 0, 0]), RET_ALLOW, "{name}");
        }
        // FS_IOC_GETFLAGS
        assert_eq!(call(libc::SYS_ioctl, [3, 0x40086602, 0, 0, 0, 0]), RET_KILL_PROCESS);
        // BINDER_SET_CONTEXT_MGR
        assert_eq!(call(libc::SYS_ioctl, [3, 0x4004_6207, 0, 0, 0, 0]), RET_KILL_PROCESS);
        // crosvm's start: exact arguments only.
        assert_eq!(i64::from(RLIMIT_NOFILE), libc::RLIMIT_NOFILE as i64);
        assert_eq!(i64::from(MLOCK_ONFAULT), libc::MLOCK_ONFAULT as i64);
        let onfault = libc::MLOCK_ONFAULT as u64;
        assert_eq!(call(libc::SYS_mlock2, [0x1000, 4096, onfault, 0, 0, 0]), RET_ALLOW);
        assert_eq!(call(libc::SYS_mlock2, [0x1000, 4096, 0, 0, 0, 0]), RET_KILL_PROCESS);
        let (filter, tsync) = (SECCOMP_SET_MODE_FILTER as u64, SECCOMP_FILTER_FLAG_TSYNC as u64);
        assert_eq!(call(libc::SYS_seccomp, [filter, tsync, 0x1000, 0, 0, 0]), RET_ALLOW);
        assert_eq!(call(libc::SYS_seccomp, [filter, 0, 0x1000, 0, 0, 0]), RET_KILL_PROCESS);
        // SECCOMP_SET_MODE_FILTER with SECCOMP_FILTER_FLAG_NEW_LISTENER
        assert_eq!(call(libc::SYS_seccomp, [filter, 8, 0x1000, 0, 0, 0]), RET_KILL_PROCESS);
        // SECCOMP_GET_NOTIF_SIZES
        assert_eq!(call(libc::SYS_seccomp, [3, 0, 0x1000, 0, 0, 0]), RET_KILL_PROCESS);
        let nofile = libc::RLIMIT_NOFILE as u64;
        let stack = libc::RLIMIT_STACK as u64;
        assert_eq!(call(libc::SYS_getrlimit, [nofile, 0x1000, 0, 0, 0, 0]), RET_ALLOW);
        assert_eq!(call(libc::SYS_getrlimit, [stack, 0x1000, 0, 0, 0, 0]), RET_KILL_PROCESS);
        assert_eq!(call(libc::SYS_ioctl, [0, TCSETS as u64, 0, 0, 0, 0]), RET_ALLOW);
        assert_eq!(call(libc::SYS_ioctl, [3, TCSETS as u64, 0, 0, 0, 0]), RET_KILL_PROCESS);
        // TIOCSTI
        assert_eq!(call(libc::SYS_ioctl, [0, 0x5412, 0, 0, 0, 0]), RET_KILL_PROCESS);
        // TCGETS stays allowed on any descriptor, the list's commands on stdin too.
        assert_eq!(call(libc::SYS_ioctl, [0, 0x5401, 0, 0, 0, 0]), RET_ALLOW);
        assert_eq!(call(libc::SYS_ioctl, [7, 0x5401, 0, 0, 0, 0]), RET_ALLOW);
        assert_eq!(call(libc::SYS_ioctl, [0, 0xc030_6201, 0, 0, 0, 0]), RET_ALLOW);
        // The list, and nothing beyond it.
        for (nr, name) in ALLOWED {
            assert_eq!(call(*nr, [0; 6]), RET_ALLOW, "{name}");
        }
        for nr in [
            libc::SYS_ptrace,
            libc::SYS_process_vm_readv,
            libc::SYS_process_vm_writev,
            libc::SYS_mount,
            libc::SYS_unshare,
            libc::SYS_setns,
            libc::SYS_bpf,
            libc::SYS_perf_event_open,
            libc::SYS_keyctl,
            libc::SYS_add_key,
            libc::SYS_io_uring_setup,
            libc::SYS_userfaultfd,
            libc::SYS_init_module,
            libc::SYS_finit_module,
            libc::SYS_setuid,
            libc::SYS_setresuid,
        ] {
            assert_eq!(call(nr, [0; 6]), RET_KILL_PROCESS, "{nr}");
        }
        // Debuggable builds log instead, and still refuse group signals.
        let d = build(RET_LOG);
        assert_eq!(eval(&d, libc::SYS_ptrace, [0; 6]), RET_LOG);
        assert_eq!(eval(&d, libc::SYS_kill, [0, 9, 0, 0, 0, 0]), eperm);
    }

    #[test]
    fn the_test_hook_and_the_user_build_action() {
        let user = if KILL_ON_USER_BUILDS { RET_KILL_PROCESS } else { RET_LOG };
        assert_eq!(USER_BUILDS.ends_with(" kill"), KILL_ON_USER_BUILDS);
        // User builds: the switch alone, whatever the environment says.
        for test in [None, Some("user"), Some("probe-virtmgr"), Some("log")] {
            assert_eq!(action(false, test), (user, false), "{test:?}");
        }
        // Debuggable builds: log, unless the hook asks for the user-build action or the probe.
        assert_eq!(action(true, None), (RET_LOG, false));
        assert_eq!(action(true, Some("")), (RET_LOG, false));
        assert_eq!(action(true, Some("kill")), (RET_LOG, false));
        assert_eq!(action(true, Some("probe-crosvm")), (RET_LOG, false));
        assert_eq!(action(true, Some("probe-fsvmd")), (RET_LOG, false));
        assert_eq!(action(true, Some("user")), (user, false));
        assert_eq!(action(true, Some("probe-virtmgr")), (user, true));
        // The probe's call is on no list: the filter gives it the default action.
        assert_eq!(eval(&build(RET_KILL_PROCESS), PROBE_CALL, [0; 6]), RET_KILL_PROCESS);
        assert!(!ALLOWED.iter().any(|(nr, _)| *nr == PROBE_CALL));
    }

    #[test]
    fn jumps_stay_inside_the_program() {
        let p = build(RET_KILL_PROCESS);
        for (i, insn) in p.iter().enumerate() {
            if insn.code == BPF_JEQ_K || insn.code == BPF_JSET_K {
                assert!(
                    i + 1 + (insn.jt as usize) < p.len() && i + 1 + (insn.jf as usize) < p.len(),
                    "jump at {i}"
                );
            }
        }
        assert_eq!(p.last().unwrap().k, RET_KILL_PROCESS);
        let mut nrs: Vec<_> = ALLOWED.iter().map(|(n, _)| *n).collect();
        nrs.sort();
        nrs.dedup();
        assert_eq!(nrs.len(), ALLOWED.len(), "a call listed twice");
        let mut cmds: Vec<_> = IOCTLS.iter().map(|(c, _)| *c).collect();
        cmds.sort();
        cmds.dedup();
        assert_eq!(cmds.len(), IOCTLS.len(), "an ioctl listed twice");
        for nr in [
            libc::SYS_ptrace,
            libc::SYS_kill,
            libc::SYS_tkill,
            libc::SYS_clone,
            libc::SYS_socket,
            libc::SYS_ioctl,
            SYS_PIDFD_OPEN,
            libc::SYS_mount,
            libc::SYS_unshare,
            libc::SYS_bpf,
            libc::SYS_getrlimit,
            libc::SYS_mlock2,
            libc::SYS_seccomp,
        ] {
            assert!(!nrs.contains(&nr), "{nr} must not be allowed outright");
        }
    }
}
