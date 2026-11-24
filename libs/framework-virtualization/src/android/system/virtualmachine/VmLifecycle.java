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

/** DiamaneOS per-run receipt, accessed only while holding VirtualMachine's own lock. */
final class VmLifecycle {
    private IVirtualMachine mRemote;
    private Object mToken = new Object();
    private boolean mStartRequested;
    private VirtualMachine.LifecycleOutcome mOutcome = VirtualMachine.LifecycleOutcome.DEFINITELY_NEVER_LAUNCHED;

    void begin() { begin(null); }

    boolean begin(IVirtualMachine previous) {
        Object token = new Object();
        boolean hadAttempt = mStartRequested || mRemote != null || previous != null;
        if (outcome() == VirtualMachine.LifecycleOutcome.UNKNOWN) {
            throw new IllegalStateException("previous VM launch outcome is unknown");
        }
        if (hadAttempt) retireKnown(previous);
        mRemote = null;
        mToken = token;
        mStartRequested = false;
        mOutcome = VirtualMachine.LifecycleOutcome.DEFINITELY_NEVER_LAUNCHED;
        return hadAttempt;
    }

    Object token() { return mToken; }
    boolean matches(Object token) { return mToken == token; }

    void attached(IVirtualMachine remote) { mRemote = remote; }

    void startRequested() {
        mStartRequested = true;
        mOutcome = VirtualMachine.LifecycleOutcome.UNKNOWN;
    }

    VirtualMachine.LifecycleOutcome outcome() {
        if (mOutcome != VirtualMachine.LifecycleOutcome.UNKNOWN) return mOutcome;
        if (!mStartRequested) return VirtualMachine.LifecycleOutcome.DEFINITELY_NEVER_LAUNCHED;
        if (mRemote == null) return VirtualMachine.LifecycleOutcome.UNKNOWN;
        try {
            remember(mRemote.getLifecycleOutcome());
        } catch (Exception unavailable) {
            return VirtualMachine.LifecycleOutcome.UNKNOWN;
        }
        if (mOutcome != VirtualMachine.LifecycleOutcome.UNKNOWN) mRemote = null;
        return mOutcome;
    }

    VirtualMachine.LifecycleOutcome retire() {
        if (mRemote == null) return outcome();
        mStartRequested = true;
        mOutcome = VirtualMachine.LifecycleOutcome.UNKNOWN;
        try { remember(mRemote.retireAndGetLifecycleOutcome()); }
        catch (Exception unavailable) { return VirtualMachine.LifecycleOutcome.UNKNOWN; }
        return mOutcome;
    }

    private void remember(int value) {
        if (value == VirtualMachineLifecycleOutcome.DEFINITELY_NEVER_LAUNCHED) {
            mOutcome = VirtualMachine.LifecycleOutcome.DEFINITELY_NEVER_LAUNCHED;
        } else if (value == VirtualMachineLifecycleOutcome.CHILD_EXIT_OBSERVED) {
            mOutcome = VirtualMachine.LifecycleOutcome.CHILD_EXIT_OBSERVED;
        }
        if (mOutcome != VirtualMachine.LifecycleOutcome.UNKNOWN) mRemote = null;
    }

    boolean retireKnown(IVirtualMachine remote) {
        if (outcome() == VirtualMachine.LifecycleOutcome.UNKNOWN) return false;
        if (remote != null) try { remote.stop(); } catch (Exception ignored) {}
        mRemote = null;
        return true;
    }

    void preserve() {
        try { outcome(); } catch (Throwable unavailable) {}
    }
}
