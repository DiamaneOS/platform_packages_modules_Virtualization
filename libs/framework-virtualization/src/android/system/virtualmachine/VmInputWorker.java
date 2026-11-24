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

import java.util.concurrent.BlockingQueue;
import java.util.concurrent.LinkedBlockingQueue;
import java.util.function.Consumer;

/** DiamaneOS input custody with explicit worker retirement and no framework lock held during dispatch. */
final class VmInputWorker<T> implements AutoCloseable {
    private final BlockingQueue<T> mQueue = new LinkedBlockingQueue<>();
    private final Consumer<T> mDispatch;
    private final Consumer<T> mRelease;
    private final Consumer<Throwable> mError;
    private final Thread mThread;
    private volatile boolean mClosed;

    VmInputWorker(Consumer<T> dispatch, Consumer<T> release, Consumer<Throwable> error) {
        mDispatch = dispatch;
        mRelease = release;
        mError = error;
        mThread = new Thread(this::loop, "AVF-input");
        try { mThread.start(); }
        catch (Throwable failedStart) { close(); throw failedStart; }
    }

    /** True transfers the item to the worker, false or throw leaves it with the caller. */
    synchronized boolean offer(T item) {
        return !mClosed && mQueue.offer(item);
    }

    private void release(T item) {
        try { mRelease.accept(item); }
        catch (Throwable failure) { report(failure); }
    }

    private void report(Throwable failure) {
        try { mError.accept(failure); } catch (Throwable ignored) {}
    }

    private void drain() {
        T item;
        while ((item = mQueue.poll()) != null) release(item);
    }

    private void loop() {
        try {
            while (!mClosed) {
                T item;
                try { item = mQueue.take(); }
                catch (InterruptedException retired) { Thread.currentThread().interrupt(); return; }
                try { if (!mClosed) mDispatch.accept(item); }
                catch (Throwable failure) { report(failure); }
                finally { release(item); }
            }
        } finally { drain(); }
    }

    @Override public void close() {
        synchronized (this) { mClosed = true; }
        mThread.interrupt();
        drain();
    }
}
