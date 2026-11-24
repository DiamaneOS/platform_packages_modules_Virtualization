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

//! DiamaneOS per-instance evidence from the child creation and wait boundaries.

#[cfg(not(early))]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU8, Ordering};

const UNATTEMPTED: u8 = 0;
const ATTEMPTING: u8 = 1;
const NEVER_LAUNCHED: u8 = 2;
const CHILD_CREATED: u8 = 3;
const EXIT_OBSERVED: u8 = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Unknown,
    NeverLaunched,
    ChildExitObserved,
}

#[derive(Default)]
pub struct LaunchEvidence {
    phase: AtomicU8,
    #[cfg(not(early))]
    retirement_claimed: AtomicBool,
}

impl LaunchEvidence {
    // One native handler claims the instance before any fallible launch setup.
    pub fn claim_start(&self) -> bool {
        self.phase
            .compare_exchange(UNATTEMPTED, ATTEMPTING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    // Called only after NotStarted is consumed under the VM-state lock, preventing a later spawn.
    pub fn cancel_unstarted(&self) {
        let mut phase = self.phase.load(Ordering::Acquire);
        while phase == UNATTEMPTED || phase == ATTEMPTING {
            match self.phase.compare_exchange(
                phase,
                NEVER_LAUNCHED,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(actual) => phase = actual,
            }
        }
    }

    pub fn child_created(&self) {
        self.phase.store(CHILD_CREATED, Ordering::Release);
    }

    pub fn child_exit_observed(&self) {
        let _ = self.phase.compare_exchange(
            CHILD_CREATED,
            EXIT_OBSERVED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    // A lost unregister reply cannot permit a second request against a reused CID.
    #[cfg(not(early))]
    pub fn claim_retirement(&self) -> bool {
        self.outcome() == Outcome::NeverLaunched
            && !self.retirement_claimed.swap(true, Ordering::AcqRel)
    }

    pub fn outcome(&self) -> Outcome {
        match self.phase.load(Ordering::Acquire) {
            NEVER_LAUNCHED => Outcome::NeverLaunched,
            EXIT_OBSERVED => Outcome::ChildExitObserved,
            _ => Outcome::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    #[test]
    fn an_unissued_or_pending_request_is_unknown() {
        let receipt = LaunchEvidence::default();
        assert_eq!(receipt.outcome(), Outcome::Unknown);
        assert!(receipt.claim_start());
        assert_eq!(receipt.outcome(), Outcome::Unknown);
    }

    #[test]
    fn dropping_local_scope_does_not_publish_a_terminal_receipt() {
        let receipt = LaunchEvidence::default();
        {
            assert!(receipt.claim_start());
        }
        assert_eq!(receipt.outcome(), Outcome::Unknown);
    }

    #[test]
    fn a_consumed_no_child_attempt_is_positive() {
        let receipt = LaunchEvidence::default();
        assert!(receipt.claim_start());
        receipt.cancel_unstarted();
        assert_eq!(receipt.outcome(), Outcome::NeverLaunched);
    }

    #[test]
    fn a_child_created_before_an_error_remains_unknown() {
        let receipt = LaunchEvidence::default();
        assert!(receipt.claim_start());
        receipt.child_created();
        assert_eq!(receipt.outcome(), Outcome::Unknown);
    }

    #[test]
    fn only_observed_exit_resolves_a_created_child() {
        let receipt = LaunchEvidence::default();
        assert!(receipt.claim_start());
        receipt.child_created();
        assert_eq!(receipt.outcome(), Outcome::Unknown);
        receipt.child_exit_observed();
        assert_eq!(receipt.outcome(), Outcome::ChildExitObserved);
        assert!(!receipt.claim_start());
    }

    #[test]
    fn an_exit_without_a_created_child_is_not_evidence() {
        let receipt = LaunchEvidence::default();
        receipt.child_exit_observed();
        assert_eq!(receipt.outcome(), Outcome::Unknown);
    }

    #[test]
    fn a_second_request_cannot_claim_or_seal_a_pending_attempt() {
        let receipt = LaunchEvidence::default();
        assert!(receipt.claim_start());
        assert!(!receipt.claim_start());
        assert_eq!(receipt.outcome(), Outcome::Unknown);
        receipt.cancel_unstarted();
        assert_eq!(receipt.outcome(), Outcome::NeverLaunched);
    }

    #[test]
    fn a_published_no_child_receipt_is_terminal() {
        let receipt = LaunchEvidence::default();
        assert!(receipt.claim_start());
        receipt.cancel_unstarted();
        assert!(!receipt.claim_start());
        assert_eq!(receipt.outcome(), Outcome::NeverLaunched);
    }

    #[test]
    fn cancellation_before_handler_entry_refuses_a_late_start() {
        let receipt = LaunchEvidence::default();
        receipt.cancel_unstarted();
        assert!(!receipt.claim_start());
        assert_eq!(receipt.outcome(), Outcome::NeverLaunched);
    }

    #[test]
    fn concurrent_queries_never_invent_a_completed_launch() {
        let receipt = Arc::new(LaunchEvidence::default());
        let barrier = Arc::new(Barrier::new(2));
        let other = receipt.clone();
        let gate = barrier.clone();
        let worker = thread::spawn(move || {
            assert!(other.claim_start());
            gate.wait();
            gate.wait();
            other.child_created();
        });
        barrier.wait();
        for _ in 0..1000 {
            assert_eq!(receipt.outcome(), Outcome::Unknown);
        }
        barrier.wait();
        worker.join().unwrap();
        assert_eq!(receipt.outcome(), Outcome::Unknown);
        receipt.child_exit_observed();
        assert_eq!(receipt.outcome(), Outcome::ChildExitObserved);
    }

    #[test]
    fn cancellation_cannot_reclassify_a_created_child() {
        let receipt = LaunchEvidence::default();
        assert!(receipt.claim_start());
        receipt.child_created();
        receipt.cancel_unstarted();
        assert_eq!(receipt.outcome(), Outcome::Unknown);
        receipt.child_exit_observed();
        receipt.cancel_unstarted();
        assert_eq!(receipt.outcome(), Outcome::ChildExitObserved);
    }

    #[test]
    fn concurrent_handlers_obtain_exactly_one_launch_claim() {
        let receipt = Arc::new(LaunchEvidence::default());
        let workers: Vec<_> = (0..32)
            .map(|_| {
                let receipt = receipt.clone();
                thread::spawn(move || receipt.claim_start())
            })
            .collect();
        assert_eq!(workers.into_iter().map(|w| usize::from(w.join().unwrap())).sum::<usize>(), 1);
    }

    #[test]
    #[cfg(not(early))]
    fn no_child_registry_retirement_has_one_instance_scoped_claim() {
        let receipt = Arc::new(LaunchEvidence::default());
        assert!(!receipt.claim_retirement());
        assert!(receipt.claim_start());
        receipt.cancel_unstarted();
        let workers: Vec<_> = (0..32)
            .map(|_| {
                let receipt = receipt.clone();
                thread::spawn(move || receipt.claim_retirement())
            })
            .collect();
        assert_eq!(workers.into_iter().map(|w| usize::from(w.join().unwrap())).sum::<usize>(), 1);
        assert!(!receipt.claim_start());
        assert!(!receipt.claim_retirement());
    }
}
