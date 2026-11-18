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

package android.os;

import java.util.HashMap;
import java.util.Map;

/**
 * Host test stand-in for the framework's PersistableBundle: the typed get and put methods
 * VirtualMachineCustomImageConfig uses, with the framework's defaults for a missing key.
 */
public final class PersistableBundle {
    private final Map<String, Object> mMap = new HashMap<>();

    private <T> T get(String key, Class<T> type, T missing) {
        Object v = mMap.get(key);
        return type.isInstance(v) ? type.cast(v) : missing;
    }

    public void putString(String key, String value) { mMap.put(key, value); }
    public String getString(String key) { return get(key, String.class, null); }
    public void putStringArray(String key, String[] value) { mMap.put(key, value); }
    public String[] getStringArray(String key) { return get(key, String[].class, null); }
    public void putInt(String key, int value) { mMap.put(key, value); }
    public int getInt(String key) { return getInt(key, 0); }
    public int getInt(String key, int missing) { return get(key, Integer.class, missing); }
    public void putBoolean(String key, boolean value) { mMap.put(key, value); }
    public boolean getBoolean(String key) { return getBoolean(key, false); }
    public boolean getBoolean(String key, boolean missing) { return get(key, Boolean.class, missing); }
    public void putBooleanArray(String key, boolean[] value) { mMap.put(key, value); }
    public boolean[] getBooleanArray(String key) { return get(key, boolean[].class, null); }
    public void putPersistableBundle(String key, PersistableBundle value) { mMap.put(key, value); }
    public PersistableBundle getPersistableBundle(String key) { return get(key, PersistableBundle.class, null); }
}
