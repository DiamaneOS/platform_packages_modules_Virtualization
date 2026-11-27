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

"""DiamaneOS: sign_virt_apex's steps for the protected VM guests, on a throwaway tree.

Both guest kernels must be in the APEX (empty only where the VMs are not built), each re-signed kernel must carry the
signing key and its initramfs digest in both descriptors, and the same checks hold for the decode service VM and the
filesystem VM. A kernel signed with another key or over another initramfs, a pair in the other guest's place and a
kernel with other descriptors are refused. Needs avbtool (or AVBTOOL, the path of avbtool.py) and openssl. The test
keys sit next to this file.
"""

import os
import hashlib
import shutil
import stat
import sys
import tempfile
import unittest
from unittest import mock

import sign_virt_apex as sva

HERE = os.path.dirname(os.path.abspath(__file__))
KEY = os.path.join(HERE, 'test.com.android.virt.pem')
OTHER_KEY = os.path.join(HERE, 'test2.com.android.virt.pem')
ROLES = ['decoder_vm', 'fsvm']


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


def cpio(names):
    """A newc cpio archive of these names (directories and empty files) and some file data."""
    out = b''
    for ino, name in enumerate(list(names) + ['TRAILER!!!']):
        role = 'decoder_vm' if 'dev/console' in names else 'fsvm'
        mode, links, major, minor = sva.guest_contract.FILES[role].get(name, (0o100755, 1, 0, 0))
        if name == 'TRAILER!!!':
            mode, links, major, minor = 0, 1, 0, 0
        data = b'\x7fELF' + b'x' * 4092 if mode & 0o170000 == 0o100000 else b''
        encoded = name.encode() + b'\0'
        fields = [ino, mode, 0, 0, links, 0, len(data), 0, 0, major, minor, len(encoded), 0]
        header = b'070701' + b''.join(b'%08X' % v for v in fields)
        entry = header + encoded
        entry += b'\0' * (-len(entry) % 4) + data
        out += entry + b'\0' * (-len(entry) % 4)
    return out + b'\0' * (-len(out) % 512)


class GuestVmSigningTest(unittest.TestCase):

    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix='sign_virt_apex_guest-')
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
        digests = {role: (len(data), hashlib.sha256(data).hexdigest()) for role in ROLES
                   for data in [cpio(sorted(sva.GUEST_VM_INITRD_FILES[role]))]}
        self.digest_patch = mock.patch.object(sva, 'ROLE_DIGESTS', digests)
        self.digest_patch.start()
        self.tree = os.path.join(self.dir, 'apex')
        os.makedirs(os.path.join(self.tree, 'etc/fs'))

    def tearDown(self):
        self.digest_patch.stop()
        os.environ['PATH'] = self.old_path
        shutil.rmtree(self.dir)

    def path(self, role, part):
        kernel_key, kernel_path, initrd_key, initrd_path = sva.GUEST_VMS[role]
        return os.path.join(self.tree, kernel_path if part == 'kernel' else initrd_path)

    def make_pair(self, role, key=KEY, files_of=None, extra_descriptor=False):
        """A pair signed like <role>_kernel_signed: boot, plus both initramfs descriptors, over an initramfs with the
        files of `files_of` (default: its own role's)."""
        kernel, initrd = self.path(role, 'kernel'), self.path(role, 'initrd')
        with open(kernel, 'wb') as f:
            f.write(os.urandom(256 * 1024))
        with open(initrd, 'wb') as f:
            f.write(cpio(sorted(sva.GUEST_VM_INITRD_FILES[files_of or role])))
        normal = os.path.join(self.dir, role + '-normal.desc')
        debug = os.path.join(self.dir, role + '-debug.desc')
        sva.GenVbmetaImage(self.args, initrd, normal, 'initrd_normal', '11' * 32)
        sva.GenVbmetaImage(self.args, initrd, debug, 'initrd_debug', '22' * 32)
        cmd = ['avbtool', 'add_hash_footer', '--key', key, '--algorithm', 'SHA256_RSA4096',
               '--partition_name', 'boot', '--salt', '33' * 32, '--partition_size', str(1024 * 1024),
               '--image', kernel, '--include_descriptors_from_image', normal,
               '--include_descriptors_from_image', debug]
        if extra_descriptor:
            extra = os.path.join(self.dir, role + '-extra.desc')
            sva.GenVbmetaImage(self.args, initrd, extra, 'vendor_boot', '44' * 32)
            cmd += ['--include_descriptors_from_image', extra]
        sva.RunCommand(self.args, cmd)
        return kernel, initrd

    def make_service_vm(self):
        with open(os.path.join(self.tree, sva.virt_apex_non_gki_files['service_vm']), 'wb') as f:
            f.write(b'service vm')

    # Which files are signed.

    def test_contract_is_the_copy_diamaneos_tools_vendors(self):
        # diamaneos-tools vendors guest_vm_contract.py byte for byte and its test_guest_contract pins the same
        # digest: a change to one copy fails here or there until both copies are the same bytes again.
        path = os.path.join(os.path.dirname(os.path.abspath(sva.__file__)), 'guest_vm_contract.py')
        with open(path, 'rb') as f:
            self.assertEqual(hashlib.sha256(f.read()).hexdigest(),
                             'fe8ceadd3051366e13396aac95ba559f930dd99e79c115d06250fc3e48a4ee33')

    def test_missing_kernel_is_an_error(self):
        for role in ROLES:
            for other in ROLES:
                open(self.path(other, 'kernel'), 'wb').close()
            os.remove(self.path(role, 'kernel'))
            with self.assertRaises(ValueError, msg=role):
                sva.GuestVmFiles(self.tree)

    def test_empty_kernels_sign_nothing_outside_arm64(self):
        for role in ROLES:
            open(self.path(role, 'kernel'), 'wb').close()
        self.assertEqual(sva.GuestVmFiles(self.tree), {})

    def test_empty_kernel_in_an_arm64_apex_is_an_error(self):
        self.make_service_vm()
        self.make_pair('decoder_vm')
        open(self.path('fsvm', 'kernel'), 'wb').close()
        with self.assertRaises(ValueError):
            sva.GuestVmFiles(self.tree)

    def test_kernel_without_initramfs_is_an_error(self):
        for role in ROLES:
            self.make_pair(role)
        for role in ROLES:
            initrd = self.path(role, 'initrd')
            saved = initrd + '.saved'
            os.rename(initrd, saved)
            with self.assertRaises(ValueError, msg=role):
                sva.GuestVmFiles(self.tree)
            os.rename(saved, initrd)

    def test_both_pairs_are_signed(self):
        self.make_service_vm()
        for role in ROLES:
            self.make_pair(role)
        files = sva.GuestVmFiles(self.tree)
        self.assertEqual(files, {
            'decoder_vm_kernel': self.path('decoder_vm', 'kernel'),
            'decoder_vm_initrd': self.path('decoder_vm', 'initrd'),
            'fsvm_kernel': self.path('fsvm', 'kernel'),
            'fsvm_initrd': self.path('fsvm', 'initrd'),
        })
        self.assertTrue(sva.GUEST_VM_KEYS == set(files))

    # The pair itself.

    def test_both_pairs_verify(self):
        # VT-4: the filesystem VM's pair verifies as the decode service VM's does.
        for role in ROLES:
            self.make_pair(role)
        files = sva.GuestVmFiles(self.tree)
        sva.VerifyGuestVms(self.args, KEY, files)
        sva.AwaitAll(sva.tasks)
        for role in ROLES:
            sva.VerifyGuestVm(self.args, role, KEY, self.path(role, 'kernel'), self.path(role, 'initrd'))
            sva.VerifyGuestVm(self.args, role, None, self.path(role, 'kernel'), self.path(role, 'initrd'))

    def test_resigned_kernel_carries_the_new_key(self):
        # As SignVirtApex does it: fresh descriptors of the unchanged initramfs, then the footer with the new key.
        for role in ROLES:
            kernel, initrd = self.make_pair(role)
            normal = os.path.join(self.dir, 'normal2.desc')
            debug = os.path.join(self.dir, 'debug2.desc')
            sva.GenVbmetaImage(self.args, initrd, normal, 'initrd_normal', '11' * 32)
            sva.GenVbmetaImage(self.args, initrd, debug, 'initrd_debug', '22' * 32)
            sva.AddHashFooter(self.args, OTHER_KEY, kernel, additional_images=[normal, debug])
            sva.VerifyGuestVm(self.args, role, OTHER_KEY, kernel, initrd)
            with self.assertRaises(ValueError, msg=role):
                sva.VerifyGuestVm(self.args, role, KEY, kernel, initrd)

    def test_other_initramfs_is_refused(self):
        for role in ROLES:
            kernel, initrd = self.make_pair(role)
            with open(initrd, 'r+b') as f:
                f.seek(200)
                first = f.read(1)
                f.seek(200)
                f.write(bytes([first[0] ^ 1]))
            # Before signing (the digests the build made) and after.
            for key in (None, KEY):
                with self.assertRaises(ValueError, msg=f'{role} {key}'):
                    sva.VerifyGuestVm(self.args, role, key, kernel, initrd)

    def test_a_pair_in_the_other_guests_place_is_refused(self):
        # Valid pairs, each signed over its own initramfs, but each with the other guest's files.
        self.make_pair('decoder_vm', files_of='fsvm')
        self.make_pair('fsvm', files_of='decoder_vm')
        for role in ROLES:
            with self.assertRaisesRegex(ValueError, f'not the {role} initramfs', msg=role):
                sva.VerifyGuestVm(self.args, role, KEY, self.path(role, 'kernel'), self.path(role, 'initrd'))

    def test_another_descriptor_is_refused(self):
        for role in ROLES:
            kernel, initrd = self.make_pair(role, extra_descriptor=True)
            with self.assertRaisesRegex(ValueError, 'not one each', msg=role):
                sva.VerifyGuestVm(self.args, role, KEY, kernel, initrd)

    def test_a_duplicate_descriptor_is_refused(self):
        # avbtool keeps one descriptor of each name when it builds a footer: the parsed descriptors stand for a
        # vbmeta made otherwise.
        kernel, initrd = self.make_pair('fsvm')
        info, descriptors = sva.AvbInfo(self.args, kernel)
        twice = descriptors + [d for d in descriptors if d[1].get('Partition Name') == 'initrd_normal']
        with mock.patch.object(sva, 'AvbInfo', return_value=(info, twice)):
            with self.assertRaisesRegex(ValueError, 'not one each'):
                sva.VerifyGuestVm(self.args, 'fsvm', KEY, kernel, initrd)

    def test_command_failure_is_not_optimized_away(self):
        with self.assertRaises(ValueError):
            sva.RunCommand(self.args, ['/bin/sh', '-c', 'exit 19'])

    def test_valid_same_layout_replacement_is_refused(self):
        kernel, initrd = self.make_pair('fsvm')
        with open(initrd, 'rb') as stream:
            replacement = stream.read().replace(b'ELFxxxx', b'ELFyyyy', 1)
        with open(initrd, 'wb') as stream:
            stream.write(replacement)
        normal, debug = os.path.join(self.dir, 'replacement-normal'), os.path.join(self.dir, 'replacement-debug')
        sva.GenVbmetaImage(self.args, initrd, normal, 'initrd_normal', '11' * 32)
        sva.GenVbmetaImage(self.args, initrd, debug, 'initrd_debug', '22' * 32)
        sva.AddHashFooter(self.args, OTHER_KEY, kernel, additional_images=[normal, debug])
        with self.assertRaisesRegex(ValueError, 'independent build role digest'):
            sva.VerifyGuestVm(self.args, 'fsvm', OTHER_KEY, kernel, initrd)

    def test_archive_bounds_and_identity(self):
        good = cpio(sorted(sva.GUEST_VM_INITRD_FILES['fsvm']))
        duplicate = cpio(sorted(sva.GUEST_VM_INITRD_FILES['fsvm']) + ['init'])
        trailer = good.index(b'TRAILER!!!') - 110
        mutations = [duplicate, good[:trailer],
                     good[:trailer + 54] + b'00000100' + good[trailer + 62:],
                     good[:110] + good[110:].replace(b'bin\0', b'binX', 1),
                     good[:54] + b'7fffffff' + good[62:]]
        for data in mutations:
            with self.subTest(data=data[:110]):
                path = self.path('fsvm', 'initrd')
                with open(path, 'wb') as stream:
                    stream.write(data)
                with self.assertRaises(ValueError):
                    sva.InitrdFiles(path)

    def test_data_after_the_trailer_is_refused(self):
        kernel, initrd = self.make_pair('fsvm')
        with open(initrd, 'ab') as f:
            f.write(cpio(['evil']))
        with self.assertRaises(ValueError):
            sva.InitrdFiles(initrd)


if __name__ == '__main__':
    unittest.main(verbosity=2)
