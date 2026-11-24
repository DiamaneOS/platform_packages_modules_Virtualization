/*
 * Copyright (C) 2021 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

package android.system.virtualmachine;

import android.system.virtualizationservice.IVirtualMachine;
import android.system.virtualizationservice.VirtualMachineLifecycleOutcome;
import android.os.ParcelFileDescriptor;
import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicIntegerArray;
import org.junit.Test;

/** The shipped helpers, including real worker Threads and real host descriptor validity. */
public class VmLifecycleHostTest {
    private static int sPass;
    private static void check(String name, boolean ok) {
        if (!ok) throw new AssertionError(name);
        sPass++;
        System.out.println("PASS " + name);
    }
    static final class Remote implements IVirtualMachine {
        int value, queries, stops, retireValue;
        boolean retireLost;
        boolean lost;
        @Override public void stop() throws Exception { stops++; if (lost) throw new IOException("stop unavailable"); }
        @Override public int retireAndGetLifecycleOutcome() throws Exception {
            if (retireLost) throw new IOException("retirement transport lost");
            return retireValue;
        }
        @Override public int getLifecycleOutcome() throws Exception {
            queries++;
            if (lost) throw new IOException("management contact lost");
            return value;
        }
    }
    @Test public void receipts() {
        VmLifecycle life = new VmLifecycle();
        Remote remote = new Remote();
        Object initial = life.token();
        life.begin(); life.attached(remote);
        check("a fresh run has a distinct callback identity", !life.matches(initial));
        check("a framework attempt before issuing start is definitely never launched",
                life.outcome() == VirtualMachine.LifecycleOutcome.DEFINITELY_NEVER_LAUNCHED && remote.queries == 0);
        life.startRequested();
        check("an issued request without terminal evidence is unknown",
                life.outcome() == VirtualMachine.LifecycleOutcome.UNKNOWN);
        boolean rejected = false;
        try { life.begin(); } catch (IllegalStateException expected) { rejected = true; }
        check("a new run cannot overwrite an unknown preceding instance", rejected);
        remote.value = VirtualMachineLifecycleOutcome.DEFINITELY_NEVER_LAUNCHED;
        check("the native Failed-before-child receipt permits recovery",
                life.outcome() == VirtualMachine.LifecycleOutcome.DEFINITELY_NEVER_LAUNCHED);
        remote.lost = true; life.preserve();
        check("a positive no-child receipt survives remote loss and retirement",
                life.outcome() == VirtualMachine.LifecycleOutcome.DEFINITELY_NEVER_LAUNCHED);
        life.begin(); remote = new Remote(); life.attached(remote); life.startRequested();
        remote.lost = true; life.preserve();
        check("failed-stop and management loss cannot become a child exit",
                life.outcome() == VirtualMachine.LifecycleOutcome.UNKNOWN);
        remote.lost = false; remote.value = 99;
        check("unknown future wire values remain unknown", life.outcome() == VirtualMachine.LifecycleOutcome.UNKNOWN);
        remote.value = VirtualMachineLifecycleOutcome.CHILD_EXIT_OBSERVED;
        check("a native observed child exit resolves this exact retained instance",
                life.outcome() == VirtualMachine.LifecycleOutcome.CHILD_EXIT_OBSERVED);
        remote.lost = true; life.preserve();
        Object ended = life.token();
        life.begin();
        check("an old callback token cannot match a later run even with a reused CID", !life.matches(ended));
        life.attached(remote); life.startRequested(); remote.lost = false;
        life.outcome(); remote.lost = true;
        check("a positive exit survives failed stop and dropped running references",
                life.outcome() == VirtualMachine.LifecycleOutcome.CHILD_EXIT_OBSERVED);
    }
    static int inputThreads() {
        int n = 0;
        for (Thread thread : Thread.getAllStackTraces().keySet())
            if (thread.isAlive() && thread.getName().equals("AVF-input")) n++;
        return n;
    }
    static void awaitThreads(int baseline) throws Exception {
        long end = System.nanoTime() + TimeUnit.SECONDS.toNanos(2);
        while (inputThreads() != baseline && System.nanoTime() < end) Thread.sleep(2);
        check("live framework input workers return to baseline " + baseline, inputThreads() == baseline);
    }
    @Test public void failedAttemptsReleaseWorkers() throws Exception {
        int baseline = inputThreads();
        AtomicInteger released = new AtomicInteger();
        for (int i = 0; i < 32; i++) {
            VmInputWorker<Integer> worker = new VmInputWorker<>(item -> {}, item -> released.incrementAndGet(), error -> {});
            try {
                check("failed-create worker accepted its one owned item " + i, worker.offer(i));
                throw new IOException("failed create before remote assignment");
            } catch (IOException expected) {
            } finally { worker.close(); }
            awaitThreads(baseline);
        }
        check("all failed-attempt input items release exactly once", released.get() == 32);
        VmInputWorker<Integer> successful = new VmInputWorker<>(item -> {}, item -> released.incrementAndGet(), error -> {});
        CountDownLatch delivered = new CountDownLatch(1);
        successful.close();
        VmInputWorker<Integer> replacement = new VmInputWorker<>(item -> delivered.countDown(), item -> released.incrementAndGet(), error -> {});
        check("a fresh worker works after repeated failed attempts", replacement.offer(33) && delivered.await(1, TimeUnit.SECONDS));
        replacement.close(); awaitThreads(baseline);
    }
    @Test public void concurrentRetirement() throws Exception {
        int baseline = inputThreads();
        Object frameworkLock = new Object();
        CountDownLatch dispatch = new CountDownLatch(1);
        AtomicIntegerArray releases = new AtomicIntegerArray(64);
        VmInputWorker<Integer> worker = new VmInputWorker<>(item -> {
            dispatch.countDown();
            synchronized (frameworkLock) {}
            if (item == 0) throw new OutOfMemoryError("input dispatch failure");
        }, item -> releases.incrementAndGet(item), error -> { throw new OutOfMemoryError("input logging"); });
        synchronized (frameworkLock) {
            check("dispatch reaches the framework lock", worker.offer(0) && dispatch.await(1, TimeUnit.SECONDS));
            for (int i = 1; i < 64; i++) check("queued item admitted " + i, worker.offer(i));
            long start = System.nanoTime(); worker.close();
            check("retirement does not join a worker while holding the framework lock",
                    System.nanoTime() - start < TimeUnit.MILLISECONDS.toNanos(200));
            check("retired worker refuses new custody", !worker.offer(0));
        }
        awaitThreads(baseline);
        for (int i = 0; i < 64; i++) check("item releases exactly once " + i, releases.get(i) == 1);
    }
    @Test public void launchFiles() throws Exception {
        List<FileInputStream> opened = new ArrayList<>();
        for (int i = 0; i < 12; i++) {
            try (VmRunFiles files = new VmRunFiles()) {
                opened.add(files.track(new FileInputStream(new File("/dev/null"))));
                opened.add(files.track(new FileInputStream(new File("/dev/null"))));
                files.track((AutoCloseable) () -> { throw new OutOfMemoryError("close failure"); });
                opened.add(files.track(new FileInputStream(new File("/dev/null"))));
                throw new IOException("failed raw configuration or create");
            } catch (IOException expected) {}
        }
        for (FileInputStream file : opened) check("partially built launch descriptor is closed", !file.getFD().valid());
        try (VmRunFiles files = new VmRunFiles()) {
            ParcelFileDescriptor file = files.open(new File("/dev/null"), 0);
            check("the file guard owns an opened descriptor", file.valid());
            files.close();
            check("the file guard releases its opened descriptor", !file.valid());
        }
    }
    @Test public void knownRetirement() {
        VmLifecycle life = new VmLifecycle();
        Remote remote = new Remote();
        life.begin(); life.attached(remote);
        check("closing an unissued raw launch retires its native registration", life.retireKnown(remote) && remote.stops == 1);
        life.begin(); remote = new Remote(); life.attached(remote); life.startRequested();
        check("unknown receipt cannot authorize resolved-only retirement", !life.retireKnown(remote) && remote.stops == 0);
        remote.value = VirtualMachineLifecycleOutcome.CHILD_EXIT_OBSERVED;
        check("actual exit becomes retained evidence before close", life.outcome() == VirtualMachine.LifecycleOutcome.CHILD_EXIT_OBSERVED);
        remote.lost = true;
        check("a failed raw stop cannot erase positive evidence", life.retireKnown(remote)
                && remote.stops == 1 && life.outcome() == VirtualMachine.LifecycleOutcome.CHILD_EXIT_OBSERVED);
        Remote replacement = new Remote();
        life.begin(); life.attached(replacement); life.startRequested();
        check("a previous positive receipt cannot attest an unknown replacement", life.outcome() == VirtualMachine.LifecycleOutcome.UNKNOWN);
    }
    @Test public void debugMarker() throws Exception {
        android.system.virtualizationservice.VirtualMachineConfig marked =
                new android.system.virtualizationservice.VirtualMachineConfig("console=ttyS0 diamaneos.debug.fail_create");
        android.content.Context caller = new android.content.Context("de.diamaneos.sanitizer");
        android.os.Build.debug = false;
        VmLaunchDebug.beforeCreate(caller, "decoder", marked);
        check("a user build ignores the create fault marker", true);
        android.os.Build.debug = true;
        VmLaunchDebug.beforeCreate(new android.content.Context("another.app"), "decoder", marked);
        check("the create marker cannot affect a different caller", true);
        VmLaunchDebug.beforeCreate(caller, "filesystem", marked);
        check("the create marker cannot affect a different VM name", true);
        VmLaunchDebug.beforeCreate(caller, "decoder", new android.system.virtualizationservice.VirtualMachineConfig("diamaneos.debug.fail_create_extra"));
        check("the create marker is an exact parameter", true);
        boolean failed = false;
        try { VmLaunchDebug.beforeCreate(caller, "decoder", marked); }
        catch (VirtualMachineException expected) { failed = true; }
        check("a debug decoder can force the real pre-assignment failure", failed);
        android.os.Build.debug = false;
    }
    @Test public void exactRetirement() {
        VmLifecycle life = new VmLifecycle();
        Remote remote = new Remote();
        life.begin(); life.attached(remote); life.startRequested();
        remote.retireLost = true;
        check("an undelivered cancellation cannot seal a pending launch", life.retire() == VirtualMachine.LifecycleOutcome.UNKNOWN);
        check("the pending raw handle remains available for reconciliation", life.outcome() == VirtualMachine.LifecycleOutcome.UNKNOWN);
        remote.retireLost = false;
        remote.retireValue = VirtualMachineLifecycleOutcome.DEFINITELY_NEVER_LAUNCHED;
        check("an exact native cancellation resolves an undelivered start", life.retire() == VirtualMachine.LifecycleOutcome.DEFINITELY_NEVER_LAUNCHED);
        remote.lost = true; remote.retireLost = true;
        check("a positive cancellation survives subsequent management loss", life.outcome() == VirtualMachine.LifecycleOutcome.DEFINITELY_NEVER_LAUNCHED);
        life.begin(); remote = new Remote(); life.attached(remote); life.startRequested();
        remote.retireValue = 99;
        check("an unrecognized retirement wire result supplies no exit evidence", life.retire() == VirtualMachine.LifecycleOutcome.UNKNOWN);
        remote.retireValue = VirtualMachineLifecycleOutcome.CHILD_EXIT_OBSERVED;
        check("a created child can retire only with positive observed exit", life.retire() == VirtualMachine.LifecycleOutcome.CHILD_EXIT_OBSERVED);
    }
    @Test public void preHandlerMarker() throws Exception {
        var marked = new android.system.virtualizationservice.VirtualMachineConfig("diamaneos.debug.fail_before_handler");
        var caller = new android.content.Context("de.diamaneos.sanitizer");
        android.os.Build.debug = false;
        VmLaunchDebug.beforeStart(caller, "decoder", marked);
        check("a user build ignores the pre-handler marker", true);
        android.os.Build.debug = true;
        VmLaunchDebug.beforeStart(new android.content.Context("another.app"), "decoder", marked);
        VmLaunchDebug.beforeStart(caller, "filesystem", marked);
        VmLaunchDebug.beforeStart(caller, "decoder", new android.system.virtualizationservice.VirtualMachineConfig("diamaneos.debug.fail_before_handler_extra"));
        check("the pre-handler marker requires the exact caller, VM and word", true);
        boolean failed = false;
        try { VmLaunchDebug.beforeStart(caller, "decoder", marked); }
        catch (VirtualMachineException expected) { failed = true; }
        check("the debug decoder can fail after issuance before native handler entry", failed);
        android.os.Build.debug = false;
    }
    public static void main(String[] args) throws Exception {
        VmLifecycleHostTest test = new VmLifecycleHostTest();
        test.receipts(); test.failedAttemptsReleaseWorkers(); test.concurrentRetirement(); test.launchFiles(); test.knownRetirement(); test.debugMarker(); test.exactRetirement(); test.preHandlerMarker();
        System.out.println("DONE pass=" + sPass + " fail=0");
    }
}
