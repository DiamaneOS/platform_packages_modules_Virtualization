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

import android.os.ParcelFileDescriptor;
import java.io.File;
import java.io.FileNotFoundException;
import java.util.ArrayList;

/** DiamaneOS local launch descriptors, released after Binder has copied them or when construction fails. */
final class VmRunFiles implements AutoCloseable {
    private final ArrayList<AutoCloseable> mFiles = new ArrayList<>(16);

    <T extends AutoCloseable> T track(T file) {
        if (file == null) return null;
        boolean kept = false;
        try { mFiles.add(file); kept = true; return file; }
        finally { if (!kept) closeOne(file); }
    }

    ParcelFileDescriptor open(File path, int mode) throws FileNotFoundException {
        mFiles.ensureCapacity(mFiles.size() + 1);
        return track(ParcelFileDescriptor.open(path, mode));
    }

    ParcelFileDescriptor openOrNull(File path, int mode) {
        try { return open(path, mode); } catch (FileNotFoundException missing) { return null; }
    }

    static void closeOne(AutoCloseable file) {
        if (file != null) try { file.close(); } catch (Throwable ignored) {}
    }

    @Override public void close() {
        while (!mFiles.isEmpty()) closeOne(mFiles.remove(mFiles.size() - 1));
    }
}
