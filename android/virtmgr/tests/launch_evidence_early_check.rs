// Copyright (C) 2026 DiamaneOS
// SPDX-License-Identifier: Apache-2.0

//! DiamaneOS compile check of early-manager receipts without test-only retirement uses.

#[path = "../src/launch_evidence.rs"]
mod launch_evidence;

fn main() {
    use launch_evidence::{LaunchEvidence, Outcome};

    let evidence = LaunchEvidence::default();
    assert_eq!(evidence.outcome(), Outcome::Unknown);
    assert!(evidence.claim_start());
    evidence.cancel_unstarted();
    assert_eq!(evidence.outcome(), Outcome::NeverLaunched);
    evidence.cancel_unstarted();
    #[cfg(not(early))]
    {
        assert!(evidence.claim_retirement());
        assert!(!evidence.claim_retirement());
    }
    let evidence = LaunchEvidence::default();
    assert!(evidence.claim_start());
    evidence.child_created();
    assert_eq!(evidence.outcome(), Outcome::Unknown);
    evidence.child_exit_observed();
    assert_eq!(evidence.outcome(), Outcome::ChildExitObserved);
    println!("LIFECYCLE-VARIANT ok early={}", cfg!(early));
}
