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

import android.content.Context;
import android.os.Build;

/** DiamaneOS a root-selected test marker, inert on user builds and outside the decode service. */
final class VmLaunchDebug {
    private VmLaunchDebug() {}
    static void beforeCreate(Context context, String name,
            android.system.virtualizationservice.VirtualMachineConfig config) throws VirtualMachineException {
        if (!Build.isDebuggable() || !"decoder".equals(name)
                || !"de.diamaneos.sanitizer".equals(context.getPackageName())
                || config.getTag() != android.system.virtualizationservice.VirtualMachineConfig.rawConfig) return;
        String params = config.getRawConfig().params;
        if (params == null) return;
        for (String word : params.split("\\s+")) {
            if ("diamaneos.debug.fail_create".equals(word)) {
                throw new VirtualMachineException("debug forced create failure before remote assignment");
            }
        }
    }
    static void beforeStart(Context context, String name,
            android.system.virtualizationservice.VirtualMachineConfig config) throws VirtualMachineException {
        if (!Build.isDebuggable() || !"decoder".equals(name)
                || !"de.diamaneos.sanitizer".equals(context.getPackageName())
                || config.getTag() != android.system.virtualizationservice.VirtualMachineConfig.rawConfig) return;
        String params = config.getRawConfig().params;
        if (params == null) return;
        for (String word : params.split("\\s+")) {
            if ("diamaneos.debug.fail_before_handler".equals(word)) {
                throw new VirtualMachineException("debug start issued before native handler entry");
            }
        }
    }
}
