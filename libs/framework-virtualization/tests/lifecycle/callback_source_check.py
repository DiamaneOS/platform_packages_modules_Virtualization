from pathlib import Path
import argparse,subprocess
p=argparse.ArgumentParser()
p.add_argument('--source',required=True)
p.add_argument('--out',required=True)
p.add_argument('--javac',default='javac')
p.add_argument('--java',default='java')
a=p.parse_args()
source=Path(a.source).read_text()
start=source.index('public void onDied(int cid, int reason)')
brace=source.index('{',start)
level=1
end=brace+1
while level:
    if source[end]=='{': level+=1
    if source[end]=='}': level-=1
    end+=1
method=source[start:end]
java='''import java.lang.ref.WeakReference;
import java.util.function.BiConsumer;
public class CallbackProbe {
    static class VirtualMachine {
        Object token=new Object();
        int cleanup, delivered=-1;
        void handleStopped(int cid,int reason) { if(cid==42&&reason==9) cleanup++; }
        void handleStopped(int cid,int reason,Object key) { if(key==token) handleStopped(cid,reason); }
    }
    interface Callback { void onStopped(VirtualMachine vm,int reason); }
    final WeakReference<VirtualMachine> mVirtualMachine;
    final Object mRunToken;
    final int mode;
    CallbackProbe(VirtualMachine vm,int mode) { mVirtualMachine=new WeakReference<>(vm);mRunToken=vm.token;this.mode=mode; }
    int getTranslatedReason(int reason) { return 9; }
    void executeCallback(BiConsumer<Callback,VirtualMachine> task) {
        if(mode==0) return;
        if(mode==1) throw new java.util.concurrent.RejectedExecutionException();
        task.accept((vm,reason)->vm.delivered=reason,mVirtualMachine.get());
    }
'''+method+'''
    public static void main(String[] args) {
        int pass=0,fail=0;
        for(int mode=0;mode<4;mode++) {
            VirtualMachine vm=new VirtualMachine();
            CallbackProbe probe=new CallbackProbe(vm,mode);
            if(mode==3) vm.token=new Object();
            try { probe.onDied(42,7); } catch(java.util.concurrent.RejectedExecutionException expected) {}
            boolean ok=vm.cleanup==(mode==3?0:1)&&(mode<2||vm.delivered==9);
            System.out.println((ok?"PASS ":"FAIL ")+"translator mode="+mode+" cleanup="+vm.cleanup+" publicReason="+vm.delivered);
            if(ok) pass++;else fail++;
        }
        System.out.println("DONE pass="+pass+" fail="+fail);
        System.exit(fail==0?0:1);
    }
}
'''
out=Path(a.out);out.mkdir(parents=True,exist_ok=True)
(out/'CallbackProbe.java').write_text(java)
with (out/'javac.txt').open('wb') as log:
    subprocess.run([a.javac,'--release','21','-d',str(out),str(out/'CallbackProbe.java')],stdout=log,stderr=subprocess.STDOUT,check=True)
with (out/'run.txt').open('wb') as log:
    result=subprocess.run([a.java,'-cp',str(out),'CallbackProbe'],stdout=log,stderr=subprocess.STDOUT)
print((out/'run.txt').read_text())
raise SystemExit(result.returncode)
