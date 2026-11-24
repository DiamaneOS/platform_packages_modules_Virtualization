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

//! DiamaneOS: an app forked from the zygote has stdin, stdout and stderr on the zygote's
//! /dev/null. The VM manager's domain may not use the zygote's descriptors, so at the exec the
//! kernel swaps in its own null file and logs a denial for each of the three. Such a stream gets a
//! /dev/null this process opens instead: the same device, with the client's own label, which the
//! VM manager and its crosvm may use (virtualizationservice_use). Terminals, pipes, sockets and
//! other files stay inherited as before.

use nix::libc::{S_IFCHR, S_IFMT};
use nix::sys::stat::{fstat, stat};
use std::os::fd::RawFd;
use std::process::{Command, Stdio};

/// Gives `command` a /dev/null of this process for each standard stream that is /dev/null.
pub fn own_null_stdio(command: &mut Command) {
    let Ok(null) = stat("/dev/null") else { return };
    let is_null = |fd: RawFd| {
        fstat(fd).is_ok_and(|s| s.st_mode & S_IFMT == S_IFCHR && s.st_rdev == null.st_rdev)
    };
    if is_null(0) {
        command.stdin(Stdio::null());
    }
    if is_null(1) {
        command.stdout(Stdio::null());
    }
    if is_null(2) {
        command.stderr(Stdio::null());
    }
}
