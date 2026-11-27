// Copyright (C) 2026 DiamaneOS
// The actual cleanup helper compiles against the internal service's optional handle contract.
#[path = "../src/launch_cleanup.rs"]
mod launch_cleanup;
#[path = "../src/launch_evidence.rs"]
mod launch_evidence;

mod crosvm {
    pub enum VmState {
        NotStarted {},
        Failed,
        Running,
    }
    pub struct VmInstance {
        pub vm_state: std::sync::Mutex<VmState>,
        pub cid: u32,
        pub launch_evidence: crate::launch_evidence::LaunchEvidence,
    }
}
mod virtualmachine {
    use std::cell::RefCell;
    #[derive(Default)]
    pub struct State {
        pub available: bool,
        pub reject: bool,
        pub calls: Vec<i32>,
        pub replacement: bool,
    }
    thread_local! { pub static STATE: RefCell<State> = RefCell::new(State::default()); }
    pub struct Service;
    impl Service {
        #[allow(non_snake_case)]
        pub fn unregisterVirtualMachine(&self, cid: i32) -> Result<(), ()> {
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                s.calls.push(cid);
                s.replacement = false;
                if s.reject {
                    Err(())
                } else {
                    Ok(())
                }
            })
        }
    }
    pub fn global_service() -> Option<Service> {
        STATE.with(|s| if s.borrow().available { Some(Service) } else { None })
    }
}
fn instance() -> crosvm::VmInstance {
    crosvm::VmInstance {
        cid: 57,
        launch_evidence: Default::default(),
        vm_state: std::sync::Mutex::new(crosvm::VmState::NotStarted {}),
    }
}
#[test]
fn an_unattempted_or_created_child_never_retires_the_global_registration() {
    let vm = instance();
    virtualmachine::STATE.with(|s| s.borrow_mut().available = true);
    launch_cleanup::retire_never_launched(&vm);
    vm.launch_evidence.child_created();
    *vm.vm_state.lock().unwrap() = crosvm::VmState::Running;
    launch_cleanup::retire_never_launched(&vm);
    vm.launch_evidence.child_exit_observed();
    launch_cleanup::retire_never_launched(&vm);
    virtualmachine::STATE.with(|s| assert!(s.borrow().calls.is_empty()));
}
#[test]
fn completed_no_child_failure_retires_only_its_exact_cid() {
    let vm = instance();
    virtualmachine::STATE.with(|s| s.borrow_mut().available = true);
    let result = launch_cleanup::start(&vm, || Err::<(), _>("setup failure"));
    assert_eq!(result, Some(Err("setup failure")));
    launch_cleanup::retire_never_launched(&vm);
    virtualmachine::STATE.with(|s| assert_eq!(s.borrow().calls, [57]));
}
#[test]
fn absent_internal_service_does_not_erase_the_positive_receipt() {
    let vm = instance();
    let result = launch_cleanup::start(&vm, || Err::<(), _>("setup failure"));
    assert_eq!(result, Some(Err("setup failure")));
    launch_cleanup::retire_never_launched(&vm);
    assert_eq!(vm.launch_evidence.outcome(), launch_evidence::Outcome::NeverLaunched);
    virtualmachine::STATE.with(|s| assert!(s.borrow().calls.is_empty()));
}
#[test]
fn refused_unregister_remains_best_effort_without_erasing_the_receipt() {
    let vm = instance();
    virtualmachine::STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.available = true;
        s.reject = true;
    });
    let result = launch_cleanup::start(&vm, || Err::<(), _>("setup failure"));
    assert_eq!(result, Some(Err("setup failure")));
    launch_cleanup::retire_never_launched(&vm);
    assert_eq!(vm.launch_evidence.outcome(), launch_evidence::Outcome::NeverLaunched);
    virtualmachine::STATE.with(|s| assert_eq!(s.borrow().calls, [57]));
}

#[test]
fn a_completed_no_child_failure_is_terminal_before_registry_retirement() {
    let vm = instance();
    virtualmachine::STATE.with(|s| s.borrow_mut().available = true);
    let result = launch_cleanup::start(&vm, || Err::<(), _>("setup failure"));
    assert_eq!(result, Some(Err("setup failure")));
    launch_cleanup::retire_never_launched(&vm);
    assert!(matches!(*vm.vm_state.lock().unwrap(), crosvm::VmState::Failed));
}
#[test]
fn duplicate_retirement_cannot_unregister_a_replacement_with_the_same_cid() {
    let vm = instance();
    virtualmachine::STATE.with(|s| s.borrow_mut().available = true);
    let result = launch_cleanup::start(&vm, || Err::<(), _>("setup failure"));
    assert_eq!(result, Some(Err("setup failure")));
    launch_cleanup::retire_never_launched(&vm);
    virtualmachine::STATE.with(|s| s.borrow_mut().replacement = true);
    launch_cleanup::retire_never_launched(&vm);
    virtualmachine::STATE.with(|s| {
        let s = s.borrow();
        assert_eq!(s.calls, [57]);
        assert!(s.replacement);
    });
}
#[test]
fn ambiguous_unregister_failure_cannot_be_retried_by_cid() {
    let vm = instance();
    virtualmachine::STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.available = true;
        s.reject = true;
    });
    let result = launch_cleanup::start(&vm, || Err::<(), _>("setup failure"));
    assert_eq!(result, Some(Err("setup failure")));
    launch_cleanup::retire_never_launched(&vm);
    virtualmachine::STATE.with(|s| s.borrow_mut().replacement = true);
    launch_cleanup::retire_never_launched(&vm);
    virtualmachine::STATE.with(|s| {
        let s = s.borrow();
        assert_eq!(s.calls, [57]);
        assert!(s.replacement);
    });
}
