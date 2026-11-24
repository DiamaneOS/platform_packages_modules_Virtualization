from pathlib import Path
import argparse,subprocess
p=argparse.ArgumentParser()
p.add_argument('--source',required=True)
p.add_argument('--out',required=True)
p.add_argument('--javac',default='javac')
p.add_argument('--java',default='java')
a=p.parse_args();source=Path(a.source).read_text();start=source.index('public void close()');brace=source.index('{',start);at=brace+1;level=1
while level:
 if source[at]=='{':level+=1
 elif source[at]=='}':level-=1
 at+=1
method=source[start:at]
java='''import java.util.concurrent.*;
public class CloseProbe implements AutoCloseable {
 static final String TAG="probe";
 static class Log { static void i(String tag,String message,Exception e) {} }
 static class Resource { volatile boolean closed; void close() {closed=true;} }
 static class Service { Service getBinder(){return this;} Service asBinder(){return this;} void unlinkToDeath(Object o,int flags){} }
 static class Lifecycle { boolean retireKnown(Object ignored){return false;} }
 final Object mLock=new Object();
 final Lifecycle mLifecycle=new Lifecycle();
 final Object mVirtualMachine=new Object(),mVSDeathRecipient=new Object();
 Service mVirtualizationService;
 Resource worker=new Resource(),descriptor=new Resource();
 final CountDownLatch stopped=new CountDownLatch(1),attempted=new CountDownLatch(1);
 boolean failStop;
 void stop() throws Exception {
  synchronized(mLock) {}
  stopped.countDown();
  if(!attempted.await(2,TimeUnit.SECONDS))throw new AssertionError("rerun never attempted");
  Thread.sleep(60);
  if(failStop)throw new Exception("stop failed");
 }
 void dropVm(){releaseRunResources();}
 void releaseRunResources(){worker.close();descriptor.close();}
'''+method+'''
 public static void main(String[] args)throws Exception {
  int pass=0,fail=0;
  for(boolean stopFails:new boolean[]{false,true}) {
   CloseProbe vm=new CloseProbe();vm.failStop=stopFails;
   Resource previousWorker=vm.worker,previousDescriptor=vm.descriptor;
   Thread close=new Thread(vm::close);
   Thread rerun=new Thread(()->{
    try {if(!vm.stopped.await(2,TimeUnit.SECONDS))throw new AssertionError("stop never returned");}
    catch(InterruptedException e){throw new AssertionError(e);}
    vm.attempted.countDown();
    synchronized(vm.mLock){vm.worker=new Resource();vm.descriptor=new Resource();}
   });
   close.start();rerun.start();close.join(3000);rerun.join(3000);
   if(close.isAlive()||rerun.isAlive())throw new AssertionError("close or rerun stalled");
   boolean replacementOpen=!vm.worker.closed&&!vm.descriptor.closed;
   boolean previousClosed=previousWorker.closed&&previousDescriptor.closed;
   for(boolean ok:new boolean[]{replacementOpen,previousClosed}) {
    System.out.println((ok?"PASS ":"FAIL ")+"stopFails="+stopFails+" replacementOpen="+replacementOpen+" previousClosed="+previousClosed);
    if(ok)pass++;else fail++;
   }
  }
  System.out.println("CLOSE DONE pass="+pass+" fail="+fail);
  System.exit(fail==0?0:1);
 }
}
'''
out=Path(a.out);out.mkdir(parents=True,exist_ok=True);(out/'CloseProbe.java').write_text(java)
with (out/'compile.txt').open('w') as f:sp=subprocess.run([a.javac,'--release','21','-d',str(out),str(out/'CloseProbe.java')],stdout=f,stderr=subprocess.STDOUT)
if sp.returncode:raise SystemExit(sp.returncode)
result=subprocess.run([a.java,'-cp',str(out),'CloseProbe'],capture_output=True,text=True);(out/'run.txt').write_text(result.stdout+result.stderr);print(result.stdout);raise SystemExit(result.returncode)
