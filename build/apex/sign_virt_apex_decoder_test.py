#!/usr/bin/env python3
#
# Copyright (C) 2026 DiamaneOS
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

"""DiamaneOS: sign_virt_apex's steps for the decode service VM, on a throwaway tree.

The decoder kernel must be in the APEX (empty only where the VM is not built), the re-signed kernel must carry the
signing key and the initramfs digest in both descriptors, and a kernel signed with another key or over another
initramfs is refused. Needs avbtool (or AVBTOOL, the path of avbtool.py) and openssl. The test keys sit next to
this file.
"""

import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest

import sign_virt_apex as sva

HERE = os.path.dirname(os.path.abspath(__file__))
KEY = os.path.join(HERE, 'test.com.android.virt.pem')
OTHER_KEY = os.path.join(HERE, 'test2.com.android.virt.pem')


class Args:
    """The defaults of ParseArgs."""
    verbose = False
    avbtool = 'avbtool'
    signing_args = None
    key_overrides = {}
    do_not_update_bootconfigs = False
    do_not_validate_avb_version = False


def write_tool(bin_dir, name, body):
    path = os.path.join(bin_dir, name)
    with open(path, 'w') as f:
        f.write('#!/bin/sh\n' + body + '\n')
    os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR)


class DecoderVmSigningTest(unittest.TestCase):

    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix='sign_virt_apex_decoder-')
        bin_dir = os.path.join(self.dir, 'bin')
        os.mkdir(bin_dir)
        avbtool = os.environ.get('AVBTOOL')
        if avbtool:
            write_tool(bin_dir, 'avbtool', f'exec "{sys.executable}" "{avbtool}" "$@"')
        # The test kernel is not compressed: "lz4 -t" answers "unrecognized header" for it.
        write_tool(bin_dir, 'lz4', 'exit 44')
        self.old_path = os.environ['PATH']
        os.environ['PATH'] = bin_dir + os.pathsep + self.old_path
        self.args = Args()
        self.tree = os.path.join(self.dir, 'apex')
        os.makedirs(os.path.join(self.tree, 'etc/fs'))
        self.kernel = os.path.join(self.tree, 'etc/fs/decoder_vm_kernel')
        self.initrd = os.path.join(self.tree, 'etc/decoder_vm_initrd.img')

    def tearDown(self):
        os.environ['PATH'] = self.old_path
        shutil.rmtree(self.dir)

    def make_signed_kernel(self, key=KEY):
        """A kernel signed like decoder_vm_kernel_signed: boot, plus both initramfs descriptors."""
        with open(self.kernel, 'wb') as f:
            f.write(os.urandom(256 * 1024))
        with open(self.initrd, 'wb') as f:
            f.write(os.urandom(64 * 1024))
        normal = os.path.join(self.dir, 'normal.desc')
        debug = os.path.join(self.dir, 'debug.desc')
        sva.GenVbmetaImage(self.args, self.initrd, normal, 'initrd_normal', '11' * 32)
        sva.GenVbmetaImage(self.args, self.initrd, debug, 'initrd_debug', '22' * 32)
        sva.RunCommand(self.args, [
            'avbtool', 'add_hash_footer', '--key', key, '--algorithm', 'SHA256_RSA4096',
            '--partition_name', 'boot', '--salt', '33' * 32, '--partition_size', str(1024 * 1024),
            '--image', self.kernel, '--include_descriptors_from_image', normal,
            '--include_descriptors_from_image', debug])
        return normal, debug

    def test_missing_kernel_is_an_error(self):
        with self.assertRaises(ValueError):
            sva.DecoderVmFiles(self.tree)

    def test_empty_kernel_signs_nothing(self):
        open(self.kernel, 'wb').close()
        self.assertEqual(sva.DecoderVmFiles(self.tree), {})

    def test_kernel_without_initramfs_is_an_error(self):
        with open(self.kernel, 'wb') as f:
            f.write(b'kernel')
        with self.assertRaises(ValueError):
            sva.DecoderVmFiles(self.tree)

    def test_signed_kernel_verifies(self):
        self.make_signed_kernel()
        files = sva.DecoderVmFiles(self.tree)
        self.assertEqual(files, {'decoder_vm_kernel': self.kernel, 'decoder_vm_initrd': self.initrd})
        sva.VerifyDecoderVmKernel(self.args, KEY, self.kernel, self.initrd)

    def test_resigned_kernel_carries_the_new_key(self):
        # As SignVirtApex does it: fresh descriptors of the unchanged initramfs, then the footer with the new key.
        self.make_signed_kernel()
        normal = os.path.join(self.dir, 'normal2.desc')
        debug = os.path.join(self.dir, 'debug2.desc')
        sva.GenVbmetaImage(self.args, self.initrd, normal, 'initrd_normal', '11' * 32)
        sva.GenVbmetaImage(self.args, self.initrd, debug, 'initrd_debug', '22' * 32)
        sva.AddHashFooter(self.args, OTHER_KEY, self.kernel, additional_images=[normal, debug])
        sva.VerifyDecoderVmKernel(self.args, OTHER_KEY, self.kernel, self.initrd)
        with self.assertRaises(AssertionError):
            sva.VerifyDecoderVmKernel(self.args, KEY, self.kernel, self.initrd)

    def test_other_initramfs_is_refused(self):
        self.make_signed_kernel()
        with open(self.initrd, 'r+b') as f:
            first = f.read(1)
            f.seek(0)
            f.write(bytes([first[0] ^ 1]))
        with self.assertRaises(AssertionError):
            sva.VerifyDecoderVmKernel(self.args, KEY, self.kernel, self.initrd)


if __name__ == '__main__':
    unittest.main(verbosity=2)
