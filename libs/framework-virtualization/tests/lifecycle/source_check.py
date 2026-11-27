from pathlib import Path
import json,argparse
parser=argparse.ArgumentParser()
parser.add_argument('--virt-root',default=str(Path(__file__).resolve().parents[4]))
parser.add_argument('--san-root',required=True)
parser.add_argument('--out',required=True)
args=parser.parse_args()
v=Path(args.virt_root)
s=Path(args.san_root)
frame=(v/'libs/framework-virtualization/src/android/system/virtualmachine/VirtualMachine.java').read_text()
config=(v/'libs/framework-virtualization/src/android/system/virtualmachine/VirtualMachineConfig.java').read_text()
native=(v/'android/virtmgr/src/crosvm.rs').read_text()
service=(v/'android/virtmgr/src/virtualmachine.rs').read_text()
cleanup=(v/'android/virtmgr/src/launch_cleanup.rs').read_text()
checks=[]
def check(name,ok):
    checks.append({'name':name,'pass':bool(ok)})
    if not ok: raise AssertionError(name)
def body(text,signature):
    a=text.index(signature)
    start=text.index('{',a)
    at=start+1;level=1
    while level:
        if text[at]=='{':level+=1
        elif text[at]=='}':level-=1
        at+=1
    return text[start:at]
check('no-child cleanup matches the internal optional service handle',
      'pub fn global_service() -> Option<' in service and 'if let Some(service)' in cleanup)
api=body(frame,'public LifecycleOutcome getLifecycleOutcome()')
check('public outcome enforces the existing custom VM privilege before its lock',
      api.index('enforceCallingOrSelfPermission(USE_CUSTOM_VIRTUAL_MACHINE_PERMISSION')<api.index('synchronized (mLock)'))
check('the public typed receipt uses the framework-owned record', 'return mLifecycle.outcome()' in api)
run=body(frame,'public void run() throws VirtualMachineException')
check('unknown predecessor check precedes the new management instance',run.index('mLifecycle.begin(mVirtualMachine)')<run.index('VirtualizationService.getInstance()'))
check('local raw launch files are guarded', 'try (VmRunFiles files = new VmRunFiles())' in run)
check('the exact remote is retained before CID and callback registration',run.index('mLifecycle.attached(mVirtualMachine)')<run.index('mCid = mVirtualMachine.getCid()'))
check('launch receipt becomes unknown before the start RPC',run.index('mLifecycle.startRequested()')<run.index('mVirtualMachine.start()'))
check('input worker starts only after the RPC and console setup return',run.index('new VmInputWorker')>run.index('setHostConsoleName('))
check('failed run cleanup cannot be skipped by an exception', 'finally' in run and 'if (!runSucceeded) releaseRunResources()' in run)
close=body(frame,'public void close()')
check('failed-stop close still retires host resources','finally' in close and 'releaseRunResources()' in close)
callback=body(frame,'public void onDied(int cid, int reason)')
check('framework cleanup precedes optional application callback submission',callback.index('handleStopped(cid, translatedReason, mRunToken)')<callback.index('executeCallback('))
check('public stop reasons are translated', 'cb.onStopped(current, translatedReason)' in callback)
handle=body(frame,'private void handleStopped(')
check('a reused CID cannot retire a newer run', 'mCid == cid && mLifecycle.matches(runToken)' in handle)
raw=body(config,'VirtualMachineRawConfig toVsRawConfig(VmRunFiles files)')
check('all raw descriptor opens use the same guard','ParcelFileDescriptor.open(' not in raw and 'files.open(' in raw)
rawbuild=body(frame,'createVirtualMachineConfigForRawFrom(VirtualMachineConfig vmConfig, VmRunFiles files)')
check('every remote input-socket end is guarded',rawbuild.count('files.track(pfds[1])')==5)
spawn=body(native,'fn run_vm(')
check('child creation is recorded immediately after the fallible spawn',spawn.index('SharedChild::spawn(&mut command)?')<spawn.index('launch.child_created()')<spawn.index('debug!("Spawned'))
monitor=body(native,'fn monitor_vm_exit(')
wait=monitor.index('let result = child.wait();')
check('only successful child wait records observed exit',monitor.index('Ok(status)',wait)<monitor.index('self.launch_evidence.child_exit_observed()',wait))
start=body(service,'fn start(&self) -> binder::Result<()>')
check('one native launch claim encloses all fallible handler setup',
      start.index('crate::launch_cleanup::start(&self.instance')<start.index('is_shutting_down()')
      and 'claim_start()' in cleanup and cleanup.index('claim_start()')<cleanup.index('Some(body())'))
check('no-child certainty follows consuming native state under its lock',
      cleanup.index('instance.vm_state.lock()')<cleanup.index('*state = VmState::Failed')
      <cleanup.index('instance.launch_evidence.cancel_unstarted()')<cleanup.index('drop(state)'))
query=body(service,'fn getLifecycleOutcome(')
check('internal bookkeeping keeps existing VM management authorization','check_manage_access()?' in query)
check('coarse VM state cannot supply a terminal receipt','get_state' not in query and 'launch_evidence.outcome()' in query)
check('old private field reflection is removed',not(s/'java/de/diamaneos/sanitizer/VmPlatform.java').exists()
      and 'getDeclaredField("mLock")' not in (s/'java/de/diamaneos/sanitizer/GuestVm.java').read_text()
      and 'getDeclaredField("mVirtualMachine")' not in (s/'java/de/diamaneos/sanitizer/GuestVm.java').read_text())
twin=(v/'guest/decoder_vm/apitwin/src/de/diamaneos/decodervm/DecoderVmApiTwin.java').read_text()
parameter_api=(v/'libs/framework-virtualization/src/android/system/virtualmachine/VirtualMachineCustomImageConfig.java').read_text()
parameter_method=body(parameter_api,'public Builder addParam(String param)')
consumer=(s/'java/de/diamaneos/sanitizer/GuestVm.java').read_text()
check('the APEX twin and consumer use the actual raw-builder parameter method',
      'vm.getLifecycleOutcome()' in twin and 'b.addParam("diamaneos.debug.fail_spawn")' in twin
      and 'this.params.add(param)' in parameter_method
      and 'builderClass.getMethod("addParam", String.class).invoke(b, parameter)' in consumer
      and 'getMethod("setParams"' not in consumer)
surface=(v/'libs/framework-virtualization/api/system-current.txt').read_text()
check('the non-updatable system API exports the method and three enum values',
      'LifecycleOutcome getLifecycleOutcome();' in surface and all('LifecycleOutcome '+n+';' in surface for n in ['CHILD_EXIT_OBSERVED','DEFINITELY_NEVER_LAUNCHED','UNKNOWN']))
health=(s/'java/de/diamaneos/sanitizer/HealthProvider.java').read_text()
check('the passive lifecycle branch returns before active health work', health.index('"lifecycle".equals(args[0])')<health.index('Decoder.Request ping'))
check('the diagnostic provider remains non-exported', 'android:exported="false"' in (s/'AndroidManifest.xml').read_text().split('android:name=".HealthProvider"')[1].split('/>')[0])
retire=body(frame,'public LifecycleOutcome retireAndGetLifecycleOutcome()')
check('exact-instance retirement keeps the existing custom privilege and framework lock',
      retire.index('enforceCallingOrSelfPermission(USE_CUSTOM_VIRTUAL_MACHINE_PERMISSION')<retire.index('synchronized (mLock)')
      and 'return mLifecycle.retire()' in retire)
raw_retire=body(service,'fn retireAndGetLifecycleOutcome(')
check('native retirement reaches status-independent cancellation and preserves typed evidence',
      raw_retire.index('check_manage_access()?')<raw_retire.index('self.instance.kill()')<raw_retire.index('self.getLifecycleOutcome()'))
kill=body(native,'pub fn kill(')
not_started=kill[kill.index('VmState::NotStarted { .. } =>'):]
check('an unentered native start is consumed before publishing NEVER',
      not_started.index('*vm_state_mg = VmState::Failed')<not_started.index('self.launch_evidence.cancel_unstarted()'))
check('close is serialized with rerun before public stop and final cleanup',
      close.index('synchronized (mLock)')<close.index('stop();')<close.index('releaseRunResources()'))
check('pre-handler injection follows issued state and precedes the real start proxy',
      run.index('mLifecycle.startRequested()')<run.index('VmLaunchDebug.beforeStart(')<run.index('mVirtualMachine.start()'))
check('the API twin directly compiles exact-instance retirement', 'return vm.retireAndGetLifecycleOutcome();' in twin)
check('the system API exports exact-instance retirement', 'LifecycleOutcome retireAndGetLifecycleOutcome();' in surface)
claim=(v/'android/virtmgr/src/launch_evidence.rs').read_text()
claim_body=body(claim,'pub fn claim_start(')
check('a terminal native receipt can never grant a later launch claim',
      'compare_exchange(' in claim_body and 'UNATTEMPTED' in claim_body and 'NEVER_LAUNCHED' not in claim_body)
stop=body(consumer,'private void stopClaimed(')
check('the consumer reconciles the same typed instance instead of coarse status-gated stop',
      'mVm.retireAndGetLifecycleOutcome()' in stop and 'mVm.stop()' not in stop)
observe=body((s/'java/de/diamaneos/sanitizer/VmRuns.java').read_text(),'synchronized void observe(')
check('consumer terminal admission explicitly names both positive enum constants',
      'outcome != LifecycleOutcome.DEFINITELY_NEVER_LAUNCHED' in observe
      and 'outcome != LifecycleOutcome.CHILD_EXIT_OBSERVED' in observe
      and 'else ended(' not in observe)
check('the new pre-handler phase is selected only in the existing debug gate',
      '"before-handler".equals(phase)' in (s/'java/de/diamaneos/sanitizer/DebugBuild.java').read_text()
      and 'if (!debuggable()) return new String[0];' in (s/'java/de/diamaneos/sanitizer/DebugBuild.java').read_text())
Path(args.out).write_text(json.dumps(checks,indent=2)+'\n')
print('Source integration checks:',len(checks),'passed')
