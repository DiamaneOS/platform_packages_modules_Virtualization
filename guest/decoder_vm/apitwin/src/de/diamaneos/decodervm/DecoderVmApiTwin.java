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

package de.diamaneos.decodervm;

import android.content.Context;
import android.system.virtualmachine.VirtualMachineConfig;
import android.system.virtualmachine.VirtualMachineCustomImageConfig;

/**
 * The decode service (packages/apps/DiamaneOSSanitizer, GuestVm.rawConfig) reaches the hidden raw VM API by
 * reflection, so a change upstream would show only at run time. This twin makes the same calls directly and is
 * compiled with every build of the virt APEX (decoder_vm_initrd_apex depends on it), so such a change breaks the
 * build instead. It is never installed or run. Keep it in step with GuestVm.rawConfig.
 */
final class DecoderVmApiTwin {
    private DecoderVmApiTwin() {
    }

    static VirtualMachineConfig rawConfig(Context context, String kernel, String initrd, String instance,
            long memoryBytes) {
        VirtualMachineCustomImageConfig.Builder b = new VirtualMachineCustomImageConfig.Builder();
        b.setName("decoder");
        b.setKernelPath(kernel);
        b.setInitrdPath(initrd);
        VirtualMachineCustomImageConfig.Disk disk = VirtualMachineCustomImageConfig.Disk.RWDisk(null);
        disk.addPartition(new VirtualMachineCustomImageConfig.Partition("vm-instance", instance, true, null));
        b.addDisk(disk);
        b.setCpuCount(2);
        VirtualMachineCustomImageConfig image = b.build();
        return new VirtualMachineConfig.Builder(context)
                .setCustomImageConfig(image)
                .setProtectedVm(true)
                .setDebugLevel(VirtualMachineConfig.DEBUG_LEVEL_NONE)
                .setMemoryBytes(memoryBytes)
                .setCpuTopology(VirtualMachineConfig.CPU_TOPOLOGY_ONE_CPU)
                .build();
    }
}
