#!/usr/bin/env bash

# Copyright (C) 2021 The Android Open Source Project
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#      http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

set -e
shopt -s extglob

TMP_ROOT=$(mktemp -d -t sign_virt_apex-XXXXXXXX)
TEST_DIR=$(dirname $0)

# To access host tools
PATH=$TEST_DIR:$PATH
DEBUGFS=$TEST_DIR/debugfs_static
FSCKEROFS=$TEST_DIR/fsck.erofs

# DiamaneOS: the protected VM guests' signing steps on a throwaway tree (sign_virt_apex_guest_test.py).
echo "Checking the guest VM signing steps ..."
python3 $TEST_DIR/sign_virt_apex_guest_test.py
python3 -O $TEST_DIR/sign_virt_apex_guest_test.py
python3 -O $TEST_DIR/sign_virt_apex_optimized_test.py

echo "Extracting the virt apex ..."
deapexer --debugfs_path $DEBUGFS --fsckerofs_path $FSCKEROFS \
  extract $TEST_DIR/com.android.virt.apex $TMP_ROOT

if [ "$(ls -A $TMP_ROOT/etc/fs/)" ]; then
  echo "Re-signing the contents ..."
  sign_virt_apex -v $TEST_DIR/test.com.android.virt.pem $TMP_ROOT
  echo "Verifying the contents ..."
  sign_virt_apex -v --verify $TEST_DIR/test.com.android.virt.pem $TMP_ROOT
  # DiamaneOS: both guests' pairs of this APEX verified on their own (the build's arm64 APEX has both), each
  # pair refused in the other's place and with a changed initramfs, and signing refused without a guest kernel.
  if [ -s $TMP_ROOT/etc/fs/decoder_vm_kernel ] || [ -s $TMP_ROOT/etc/fs/fsvm_kernel ]; then
    for k in decoder_vm_kernel fsvm_kernel; do
      [ -s $TMP_ROOT/etc/fs/$k ] || { echo "FAIL: $k is missing or empty next to the other guest"; exit 1; }
    done
    echo "Verifying that the guest pairs are refused in each other's place ..."
    SWAP=$(mktemp -d -t sign_virt_apex_swap-XXXXXXXX)
    cp -a $TMP_ROOT/. $SWAP/
    mv $SWAP/etc/fs/decoder_vm_kernel $SWAP/k && mv $SWAP/etc/fs/fsvm_kernel $SWAP/etc/fs/decoder_vm_kernel
    mv $SWAP/k $SWAP/etc/fs/fsvm_kernel
    mv $SWAP/etc/decoder_vm_initrd.img $SWAP/i && mv $SWAP/etc/fsvm_initrd.img $SWAP/etc/decoder_vm_initrd.img
    mv $SWAP/i $SWAP/etc/fsvm_initrd.img
    if sign_virt_apex --verify $TEST_DIR/test.com.android.virt.pem $SWAP; then
      echo "FAIL: swapped guest pairs passed"; exit 1
    fi
    rm -rf $SWAP
    for initrd in decoder_vm_initrd.img fsvm_initrd.img; do
      echo "Verifying that a changed $initrd is refused ..."
      CHANGED=$(mktemp -d -t sign_virt_apex_changed-XXXXXXXX)
      cp -a $TMP_ROOT/. $CHANGED/
      printf 'x' | dd of=$CHANGED/etc/$initrd bs=1 count=1 seek=200 conv=notrunc status=none
      if sign_virt_apex --verify $TEST_DIR/test.com.android.virt.pem $CHANGED; then
        echo "FAIL: a changed $initrd passed"; exit 1
      fi
      rm -rf $CHANGED
    done
  fi
  for k in decoder_vm_kernel fsvm_kernel; do
    echo "Verifying that signing fails without $k ..."
    MISSING=$(mktemp -d -t sign_virt_apex_missing-XXXXXXXX)
    cp -a $TMP_ROOT/. $MISSING/
    rm -f $MISSING/etc/fs/$k
    if sign_virt_apex $TEST_DIR/test.com.android.virt.pem $MISSING; then
      echo "FAIL: signed without $k"; exit 1
    fi
    rm -rf $MISSING
  done
  echo "Done."
else
  echo "No filesystem images. Skip."
fi

