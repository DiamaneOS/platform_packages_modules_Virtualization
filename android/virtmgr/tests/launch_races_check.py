from pathlib import Path
import argparse,subprocess
p=argparse.ArgumentParser();p.add_argument('--virt-root',required=True);p.add_argument('--out',required=True);p.add_argument('--rustc',default='rustc');a=p.parse_args();v=Path(a.virt_root).resolve();out=Path(a.out).resolve();out.mkdir(parents=True,exist_ok=True)
s=(v/'android/virtmgr/src/virtualmachine.rs').read_text();start=s.index('fn start(&self) -> binder::Result<()>');brace=s.index('{',start);at=brace+1;level=1
while level:
 if s[at]=='{':level+=1
 elif s[at]=='}':level-=1
 at+=1
method=s[start:at]
code=r'''// Copyright (C) 2026 DiamaneOS
// SPDX-License-Identifier: Apache-2.0
extern crate self as binder;
extern crate self as scopeguard;
use std::sync::{Arc,Mutex,atomic::{AtomicBool,AtomicUsize,Ordering}};
use std::time::{Duration,Instant};
pub type Result<T> = std::result::Result<T,&'static str>;
pub struct Status;
impl Status { pub fn new_service_specific_error_str(_:i32,_:Option<&str>)->&'static str {"refused"} }
pub struct Guard<T,F:FnOnce(T)> {value:Option<T>,f:Option<F>}
pub fn guard<T,F:FnOnce(T)>(value:T,f:F)->Guard<T,F>{Guard{value:Some(value),f:Some(f)}}
impl<T,F:FnOnce(T)> Drop for Guard<T,F>{fn drop(&mut self){self.f.take().unwrap()(self.value.take().unwrap())}}
#[path="EVIDENCE"] mod launch_evidence;
#[path="CLEANUP"] mod launch_cleanup;
trait Errors<T> {
 fn context(self,_:String)->Self;
 fn with_context(self,_:impl FnOnce()->String)->Self;
 fn with_log(self)->Self;
 fn or_service_specific_exception(self,_:i32)->Self;
}
impl<T> Errors<T> for Result<T>{fn context(self,_:String)->Self{self} fn with_context(self,_:impl FnOnce()->String)->Self{self} fn with_log(self)->Self{self} fn or_service_specific_exception(self,_:i32)->Self{self}}
mod aidl {#[allow(non_camel_case_types)]pub enum DeathReason{HOST_SHUTDOWN}pub const ERROR_UNEXPECTED:i32=-1;}
struct RpcServer;
impl RpcServer {fn new_vsock(_:(),_:u32,_:u32)->Result<(Self,())>{Ok((Self,()))}fn start(&self){}}
struct VirtualMachineService;
impl VirtualMachineService {fn new_binder(_:Arc<crosvm::VmInstance>)->Self{Self}fn as_binder(self){}}
struct Callbacks;
impl Callbacks {fn callback_on_died(&self,_:u32,_:aidl::DeathReason){}}
#[derive(Default)]struct Context {setup:AtomicUsize,ready:AtomicBool,second_entered:AtomicBool,second_done:AtomicBool,spawn_pending:AtomicBool,release:AtomicBool,spawns:AtomicUsize}
thread_local!{static CURRENT:std::cell::RefCell<Option<(Arc<Context>,bool)>>=const{std::cell::RefCell::new(None)};}
fn wait_for(mut predicate:impl FnMut()->bool){let end=Instant::now()+Duration::from_secs(2);while !predicate(){assert!(Instant::now()<end,"race latch timed out");std::thread::yield_now();}}
fn is_shutting_down()->Result<bool>{CURRENT.with(|slot|{let slot=slot.borrow();let(ctx,first)=slot.as_ref().unwrap();ctx.setup.fetch_add(1,Ordering::SeqCst);if *first{ctx.ready.store(true,Ordering::SeqCst);wait_for(||ctx.second_entered.load(Ordering::SeqCst)||ctx.second_done.load(Ordering::SeqCst));Err("pre-instance setup failed")}else{Ok(false)}})}
mod virtualmachine {
 pub struct Service;
 impl Service {#[allow(non_snake_case)]pub fn unregisterVirtualMachine(&self,_:i32)->std::result::Result<(),()>{Ok(())}}
 pub fn global_service()->Option<Service>{None}
}
mod crosvm {
 use super::*;
 pub enum VmState{NotStarted{},Failed,Running}
 pub struct VmInstance {pub vm_state:Mutex<VmState>,pub launch_evidence:launch_evidence::LaunchEvidence,pub cid:u32, pub requires_vm_service:bool, pub callbacks:Callbacks, pub vm_service:Mutex<Option<RpcServer>>,pub vendor_tee_services:Vec<()>,pub ctx:Arc<Context>}
 impl VmInstance {
  pub fn start(self:&Arc<Self>)->Result<()> {
   let mut state=self.vm_state.lock().unwrap();
   if !matches!(*state,VmState::NotStarted{}){return Err("instance consumed")}
   *state=VmState::Failed;
   self.ctx.spawn_pending.store(true,Ordering::SeqCst);
   self.ctx.second_entered.store(true,Ordering::SeqCst);
   wait_for(||self.ctx.release.load(Ordering::SeqCst));
   self.launch_evidence.child_created();self.ctx.spawns.fetch_add(1,Ordering::SeqCst);
   *state=VmState::Running;self.ctx.spawn_pending.store(false,Ordering::SeqCst);Ok(())
  }
 }
}
use crosvm::VmState;
struct VirtualMachine {instance:Arc<crosvm::VmInstance>}
impl VirtualMachine {
 fn handle_vendor_tee_services(&self)->Result<()>{Ok(())}
METHOD
}
fn race()->(usize,bool,usize,bool) {
 let ctx=Arc::new(Context::default());
 let instance=Arc::new(crosvm::VmInstance{vm_state:Mutex::new(VmState::NotStarted{}),launch_evidence:Default::default(),cid:57,requires_vm_service:false,callbacks:Callbacks,vm_service:Mutex::new(None),vendor_tee_services:Vec::new(),ctx:ctx.clone()});
 let vm=Arc::new(VirtualMachine{instance});
 let first={let vm=vm.clone();let ctx=ctx.clone();std::thread::spawn(move||{CURRENT.with(|s|*s.borrow_mut()=Some((ctx,true)));let _=vm.start();})};
 wait_for(||ctx.ready.load(Ordering::SeqCst));
 let second={let vm=vm.clone();let ctx=ctx.clone();std::thread::spawn(move||{CURRENT.with(|s|*s.borrow_mut()=Some((ctx.clone(),false)));let _=vm.start();ctx.second_done.store(true,Ordering::SeqCst);})};
 wait_for(||vm.instance.launch_evidence.outcome()==launch_evidence::Outcome::NeverLaunched);
 let premature=ctx.spawn_pending.load(Ordering::SeqCst);
 ctx.release.store(true,Ordering::SeqCst);first.join().unwrap();second.join().unwrap();
 let terminal=matches!(*vm.instance.vm_state.lock().unwrap(),VmState::Failed);
 (ctx.setup.load(Ordering::SeqCst),premature,ctx.spawns.load(Ordering::SeqCst),terminal)
}
#[test]fn nonclaiming_handler_cannot_enter_fallible_setup_or_spawn(){let(calls,_,spawns,_)=race();assert_eq!((calls,spawns),(1,0),"nonclaiming handler entered the actual start body");}
#[test]fn a_reader_cannot_cache_never_while_a_start_body_can_still_spawn(){let(_,premature,spawns,terminal)=race();assert!(!premature,"reader observed NEVER during an in-progress native spawn body");assert_eq!(spawns,0);assert!(terminal,"positive publication preceded consuming the config");}
'''
def extract(signature):
 start=s.index(signature);brace=s.index('{',start);at=brace+1;level=1
 while level:
  if s[at]=='{':level+=1
  elif s[at]=='}':level-=1
  at+=1
 return s[start:at]
method += "\n"+extract('fn getLifecycleOutcome(')+"\n"+extract('fn retireAndGetLifecycleOutcome(')
code=code.replace('pub const ERROR_UNEXPECTED:i32=-1;', 'pub const ERROR_UNEXPECTED:i32=-1; #[allow(non_camel_case_types)] #[derive(Debug,PartialEq)] pub enum VirtualMachineLifecycleOutcome{UNKNOWN,DEFINITELY_NEVER_LAUNCHED,CHILD_EXIT_OBSERVED}')
code=code.replace('fn is_shutting_down()', 'fn check_manage_access()->Result<()>{Ok(())}\nfn is_shutting_down()')
code=code.replace('spawns:AtomicUsize}', 'spawns:AtomicUsize,kill_fails:AtomicBool,observe_exit:AtomicBool}')
code=code.replace('  pub fn start(self:&Arc<Self>)', """  pub fn kill(&self)->Result<()> {
   if self.ctx.kill_fails.load(Ordering::SeqCst){return Err("cancel transport failed")}
   let mut state=self.vm_state.lock().unwrap();
   match *state {
    VmState::NotStarted{}=>{*state=VmState::Failed;self.launch_evidence.cancel_unstarted();Ok(())}
    VmState::Running if self.ctx.observe_exit.load(Ordering::SeqCst)=>{self.launch_evidence.child_exit_observed();*state=VmState::Failed;Ok(())}
    _=>Err("no observed child exit")
   }
  }
  pub fn start(self:&Arc<Self>)""")
code += r'''
fn fresh()->Arc<VirtualMachine>{let ctx=Arc::new(Context::default());Arc::new(VirtualMachine{instance:Arc::new(crosvm::VmInstance{vm_state:Mutex::new(VmState::NotStarted{}),launch_evidence:Default::default(),cid:58,requires_vm_service:false,callbacks:Callbacks,vm_service:Mutex::new(None),vendor_tee_services:Vec::new(),ctx})})}
#[test]fn exact_retirement_cancels_an_unentered_handler_and_rejects_its_late_start(){let vm=fresh();assert_eq!(vm.retireAndGetLifecycleOutcome().unwrap(),aidl::VirtualMachineLifecycleOutcome::DEFINITELY_NEVER_LAUNCHED);assert!(vm.start().is_err());assert_eq!(vm.instance.ctx.setup.load(Ordering::SeqCst),0);assert_eq!(vm.instance.ctx.spawns.load(Ordering::SeqCst),0);}
#[test]fn a_lost_cancel_does_not_attest_a_subsequently_created_child(){let vm=fresh();vm.instance.ctx.kill_fails.store(true,Ordering::SeqCst);assert_eq!(vm.retireAndGetLifecycleOutcome().unwrap(),aidl::VirtualMachineLifecycleOutcome::UNKNOWN);vm.instance.ctx.release.store(true,Ordering::SeqCst);CURRENT.with(|s|*s.borrow_mut()=Some((vm.instance.ctx.clone(),false)));vm.start().unwrap();assert_eq!(vm.instance.ctx.spawns.load(Ordering::SeqCst),1);assert_eq!(vm.getLifecycleOutcome().unwrap(),aidl::VirtualMachineLifecycleOutcome::UNKNOWN);assert_eq!(vm.retireAndGetLifecycleOutcome().unwrap(),aidl::VirtualMachineLifecycleOutcome::UNKNOWN);}
#[test]fn raw_retirement_accepts_only_an_observed_created_child_exit(){let vm=fresh();vm.instance.ctx.release.store(true,Ordering::SeqCst);CURRENT.with(|s|*s.borrow_mut()=Some((vm.instance.ctx.clone(),false)));vm.start().unwrap();assert_eq!(vm.retireAndGetLifecycleOutcome().unwrap(),aidl::VirtualMachineLifecycleOutcome::UNKNOWN);vm.instance.ctx.observe_exit.store(true,Ordering::SeqCst);assert_eq!(vm.retireAndGetLifecycleOutcome().unwrap(),aidl::VirtualMachineLifecycleOutcome::CHILD_EXIT_OBSERVED);}
'''
code=code.replace('EVIDENCE',str(v/'android/virtmgr/src/launch_evidence.rs')).replace('CLEANUP',str(v/'android/virtmgr/src/launch_cleanup.rs')).replace('METHOD',method)
(out/'race.rs').write_text(code)
with (out/'compile.txt').open('w') as f:result=subprocess.run([a.rustc,'--edition=2021','--test',str(out/'race.rs'),'-o',str(out/'race')],stdout=f,stderr=subprocess.STDOUT)
if result.returncode:print((out/'compile.txt').read_text());raise SystemExit(result.returncode)
result=subprocess.run([str(out/'race'),'--test-threads=1'],capture_output=True,text=True);(out/'run.txt').write_text(result.stdout+result.stderr);print(result.stdout);raise SystemExit(result.returncode)
