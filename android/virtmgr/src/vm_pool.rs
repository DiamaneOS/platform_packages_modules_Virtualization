// Copyright 2026 The DiamaneOS Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! DiamaneOS: memory for protected VMs from the pool the kernel sets aside at boot.
//!
//! On the FP6 every page of a protected VM is lent before it starts, and the resource manager refuses a VM whose
//! memory comes in too many physically separate pieces. Once free memory is scattered, even a small VM can need that
//! many. The kernel keeps a pool away from everything else from boot (kernel.reserve_mem in the device's bootconfig)
//! and hands it out in 2 MiB blocks through /dev/gunyah_vm_pool. crosvm then maps the VM's RAM from that file
//! (`--file-backed-mapping ...,ram`), which its Gunyah backend binds with GH_VM_ANDROID_MAP_CMA_MEM.
//!
//! Who gets the pool is SELinux's decision, not a setting: only the VM managers of the decode service and of fsvmd
//! may open the node (and the shell's on debuggable builds, for tests). For every other caller, on a device without
//! a pool, or when the pool has no room, the VM gets ordinary memory as before, with the kernel's piece limit.

use log::{info, warn};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::time::{Duration, Instant};

const POOL_DEVICE: &str = "/dev/gunyah_vm_pool";

/// struct gunyah_vm_pool_mem_args, kernel_common include/uapi/linux/gunyah.h.
#[repr(C)]
struct PoolMemArgs {
    size: u64,
    flags: u64,
}

/// GH_ANDROID_CREATE_POOL_MEM_FD: _IOW('A', 0x30, struct gunyah_vm_pool_mem_args).
const GH_ANDROID_CREATE_POOL_MEM_FD: u32 = 0x4010_4130;

/// How long a start waits for room that a VM still ending holds: its crosvm gives the blocks back when it exits, after
/// the hypervisor returned them and the kernel zeroed them. A restarted decode service asked about 0.15 s before its old
/// VM was gone, and without the wait its new VM stayed on ordinary memory for good.
const ROOM_WAIT: Duration = Duration::from_secs(5);

/// A file that holds `size` bytes of pool memory for one protected VM, or None when the VM gets ordinary memory.
/// Never fails the VM: any error here means ordinary memory. A full pool is asked again, with growing pauses, for up to
/// ROOM_WAIT before that.
pub fn take(size: u64) -> Option<File> {
    let started = Instant::now();
    let mut pause = Duration::from_millis(50);
    let mut waited = false;
    loop {
        match try_take(size) {
            Taken::File(file) => {
                if waited {
                    info!("VM memory pool: room came back after {:?}", started.elapsed());
                }
                info!("VM memory pool: {} MiB of the VM's RAM from the pool", size >> 20);
                return Some(file);
            }
            Taken::NoRoom if started.elapsed() + pause <= ROOM_WAIT => {
                std::thread::sleep(pause);
                waited = true;
                pause = (pause * 2).min(Duration::from_secs(1));
            }
            Taken::NoRoom => {
                info!(
                    "VM memory pool has no room for {} MiB, the VM gets ordinary memory (waited {:?})",
                    size >> 20,
                    started.elapsed()
                );
                return None;
            }
            Taken::Never => return None,
        }
    }
}

enum Taken {
    File(File),
    /// The pool is full now.
    NoRoom,
    /// No pool for this VM: none on the device, not for this caller, too large, or an error.
    Never,
}

/// One request. Each needs its own open of the node: the kernel makes at most one file per open.
fn try_take(size: u64) -> Taken {
    // Read-only: the one ioctl needs no write access, and an app's VM manager runs with the app's MLS categories,
    // which SELinux lets read the node (s0) but not write it.
    let device = match OpenOptions::new().read(true).custom_flags(libc::O_CLOEXEC).open(POOL_DEVICE)
    {
        Ok(f) => f,
        // No pool on this device or in this build.
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Taken::Never,
        // This caller's VMs do not get the pool. SELinux refuses the open quietly, so the line says it.
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            info!("VM memory pool: not for this caller's VMs, the VM gets ordinary memory");
            return Taken::Never;
        }
        Err(e) => {
            warn!("VM memory pool: {POOL_DEVICE}: {e}, the VM gets ordinary memory");
            return Taken::Never;
        }
    };
    let args = PoolMemArgs { size, flags: 0 };
    // SAFETY: the ioctl reads one struct gunyah_vm_pool_mem_args through the pointer, which stays valid for the
    // call, and returns a new descriptor or -1. The device descriptor stays open meanwhile.
    let fd = unsafe { libc::ioctl(device.as_raw_fd(), GH_ANDROID_CREATE_POOL_MEM_FD as _, &args) };
    if fd < 0 {
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::ENOMEM) {
            return Taken::NoRoom;
        } else if e.raw_os_error() == Some(libc::EFBIG) {
            info!("VM memory pool: {} MiB is larger than a pool VM, the VM gets ordinary memory", size >> 20);
        } else {
            warn!("VM memory pool: {e}, the VM gets ordinary memory");
        }
        return Taken::Never;
    }
    // SAFETY: the kernel returned a new descriptor that nothing else owns.
    Taken::File(unsafe { File::from_raw_fd(fd) })
}
