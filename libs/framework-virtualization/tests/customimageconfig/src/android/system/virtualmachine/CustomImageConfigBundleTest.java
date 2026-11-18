/*
 * Copyright (C) 2026 DiamaneOS
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

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.fail;

import android.os.PersistableBundle;

import org.junit.Test;
import org.junit.runner.RunWith;
import org.junit.runners.JUnit4;

/**
 * DiamaneOS: a raw VM's vCPU count (setCpuCount, vm-vcpus) survives the bundle the framework stores the VM's config
 * in, and a bundle without it (a VM created before) reads as 0, the config's CPU topology.
 */
@RunWith(JUnit4.class)
public class CustomImageConfigBundleTest {
    private static VirtualMachineCustomImageConfig.Builder raw() {
        return new VirtualMachineCustomImageConfig.Builder()
                .setName("decoder")
                .setKernelPath("/apex/com.android.virt/etc/fs/decoder_vm_kernel")
                .setInitrdPath("/apex/com.android.virt/etc/decoder_vm_initrd.img");
    }

    @Test
    public void cpuCountSurvivesTheBundle() {
        VirtualMachineCustomImageConfig config = raw().setCpuCount(2).build();
        assertEquals(2, config.getCpuCount());
        VirtualMachineCustomImageConfig back =
                VirtualMachineCustomImageConfig.from(config.toPersistableBundle());
        assertEquals(2, back.getCpuCount());
        assertEquals("decoder", back.getName());
    }

    @Test
    public void noCountIsTheTopology() {
        assertEquals(0, raw().build().getCpuCount());
        PersistableBundle old = raw().build().toPersistableBundle();
        assertEquals(0, VirtualMachineCustomImageConfig.from(old).getCpuCount());
    }

    @Test
    public void negativeCountIsRefused() {
        try {
            raw().setCpuCount(-1);
            fail("a negative vCPU count was taken");
        } catch (IllegalArgumentException expected) {
            // refused
        }
    }
}
