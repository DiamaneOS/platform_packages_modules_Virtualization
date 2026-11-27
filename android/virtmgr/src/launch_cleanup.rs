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

//! DiamaneOS release the global registration for an instance with no child exit monitor.

use crate::crosvm::{VmInstance, VmState};
use crate::launch_evidence::Outcome;

struct StartCleanup<'a>(&'a VmInstance);

impl Drop for StartCleanup<'_> {
    fn drop(&mut self) {
        let instance = self.0;
        let Ok(mut state) = instance.vm_state.lock() else { return };
        match &*state {
            VmState::NotStarted { .. } => *state = VmState::Failed,
            VmState::Failed => {}
            _ => return,
        }
        instance.launch_evidence.cancel_unstarted();
        drop(state);
        retire_never_launched(instance);
    }
}

// Keep the launch claim around the complete handler body, including fallible setup.
pub fn start<T>(instance: &VmInstance, body: impl FnOnce() -> T) -> Option<T> {
    if !instance.launch_evidence.claim_start() {
        return None;
    }
    let _cleanup = StartCleanup(instance);
    Some(body())
}

pub fn retire_never_launched(instance: &VmInstance) {
    // Seal an unconsumed config before releasing the CID, never hold this lock across Binder.
    let Ok(mut state) = instance.vm_state.lock() else { return };
    if instance.launch_evidence.outcome() != Outcome::NeverLaunched {
        return;
    }
    match &*state {
        VmState::NotStarted { .. } => *state = VmState::Failed,
        VmState::Failed => {}
        _ => return,
    }
    drop(state);
    #[cfg(not(early))]
    if let Some(service) = crate::virtualmachine::global_service() {
        if instance.launch_evidence.claim_retirement() {
            let _ = service.unregisterVirtualMachine(instance.cid as i32);
        }
    }
}
