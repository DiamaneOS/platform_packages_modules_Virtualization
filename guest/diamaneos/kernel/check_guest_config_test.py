#!/usr/bin/env python3
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
"""Tests of check-guest-config.py: the published guest.config against trim.list, and what a trim list may say."""
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
CHECK = os.path.join(HERE, 'check-guest-config.py')


def run(base, guest, trim):
    return subprocess.run([sys.executable, CHECK, base, guest, trim], capture_output=True, text=True)


class CheckGuestConfigTest(unittest.TestCase):

    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.config = os.path.join(HERE, 'guest.config')

    def tearDown(self):
        self.dir.cleanup()

    def trim(self, text):
        path = os.path.join(self.dir.name, 'trim.list')
        with open(path, 'w') as f:
            f.write(text)
        return path

    def config_with(self, name, values):
        with open(self.config) as f:
            lines = f.readlines()
        path = os.path.join(self.dir.name, name)
        with open(path, 'w') as f:
            for line in lines:
                if any(line.startswith('CONFIG_' + k + '=') or
                       line.startswith('# CONFIG_' + k + ' is not set') for k in values):
                    continue
                f.write(line)
            for key, value in values.items():
                if value is not None:
                    f.write('CONFIG_%s=%s\n' % (key, value))
        return path

    def test_both_listed_hardening_upgrades_pass(self):
        options = ('ZERO_CALL_USED_REGS', 'SECURITY_DMESG_RESTRICT')
        base = self.config_with('base', dict.fromkeys(options, None))
        guest = self.config_with('guest', dict.fromkeys(options, 'y'))
        result = run(base, guest, self.trim(''))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_a_listed_hardening_downgrade_fails(self):
        for option in ('ZERO_CALL_USED_REGS', 'SECURITY_DMESG_RESTRICT'):
            for value in (None, 'n', 'm'):
                with self.subTest(option=option, value=value):
                    base = self.config_with('base', {option: 'y'})
                    guest = self.config_with('guest', {option: value})
                    result = run(base, guest, self.trim(''))
                    self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                    self.assertIn(option + ' y->', result.stdout)

    def test_an_unlisted_hardening_upgrade_fails(self):
        for option in ('PAGE_TABLE_CHECK', 'SCHED_STACK_END_CHECK'):
            with self.subTest(option=option):
                base = self.config_with('base', {option: None})
                guest = self.config_with('guest', {option: 'y'})
                result = run(base, guest, self.trim(''))
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn(option + ' None->y', result.stdout)

    def test_listed_options_still_enabled_pass(self):
        options = ('ZERO_CALL_USED_REGS', 'SECURITY_DMESG_RESTRICT')
        config = self.config_with('enabled', dict.fromkeys(options, 'y'))
        result = run(config, config, self.trim(''))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_the_published_config_keeps_only_forced_options(self):
        # The published config against itself as base: only the trim list's part of the check can fail.
        result = run(self.config, self.config, os.path.join(HERE, 'trim.list'))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('TRIMMED but still set, no reason given 0:', result.stdout)

    def test_a_trimmed_option_still_set_without_a_reason_fails(self):
        result = run(self.config, self.config, self.trim('POWER_SUPPLY\nEXT4_FS\n'))
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn('no reason given 1: POWER_SUPPLY', result.stdout)

    def test_a_reason_for_an_option_that_is_gone_fails(self):
        result = run(self.config, self.config, self.trim('EXT4_FS forced: it is not\n'))
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn('FORCED but gone 1: EXT4_FS', result.stdout)

    def test_empty_reasons_and_duplicate_options_fail(self):
        for text in ('POWER_SUPPLY forced:\n', 'POWER_SUPPLY forced:   # empty\n',
                     'POWER_SUPPLY forced: kept\nPOWER_SUPPLY\n'):
            with self.subTest(text=text):
                result = run(self.config, self.config, self.trim(text))
                self.assertNotEqual(result.returncode, 0, result.stdout)

    def test_only_a_reason_may_follow_the_name(self):
        result = run(self.config, self.config, self.trim('POWER_SUPPLY because\n'))
        self.assertNotEqual(result.returncode, 0)


if __name__ == '__main__':
    unittest.main(verbosity=2)
