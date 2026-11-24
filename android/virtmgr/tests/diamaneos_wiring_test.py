#!/usr/bin/env python3
"""Check the parser and output boundary hooks in the production call sites."""
import os
from pathlib import Path
import unittest

ROOT = Path(os.environ.get('VIRTMGR_SOURCE', Path(__file__).resolve().parents[1]))


class WiringTest(unittest.TestCase):
    def test_original_app_request_is_rejected_before_payload_conversion(self):
        source = (ROOT / 'src/virtualmachine.rs').read_text()
        create = source[source.index('let caller_secontext = getprevcon()'):]
        self.assertLess(create.index('fsvm_client::check_variant('), create.index('load_app_config('))
        self.assertLess(create.index('fsvm_client::check_variant('), create.index('maybe_create_reference_dt_overlay('))

    def test_instance_is_replaced_before_composite_parsing(self):
        source = (ROOT / 'src/crosvm.rs').read_text()
        body = source[source.index('fn add_disk_arg('):source.index('fn add_disk_arg(') + 6000]
        self.assertLess(body.index('crate::fsvm_instance::partitions('), body.index('composite::make_composite_image('))
        self.assertIn('composite::make_composite_image(\n                    partitions,', body)
        helper = (ROOT / 'src/fsvm_instance.rs').read_text()
        self.assertIn('image: Some(ParcelFileDescriptor::new(crate::fsvm_client::instance_file(directory)?))', helper)
        self.assertNotIn('.clone()', helper)

    def test_both_streams_use_the_tested_output_and_argument_selection(self):
        source = (ROOT / 'src/crosvm.rs').read_text()
        for method, field, end in [('add_console_arg', 'console_out', 'add_log_arg'),
                                   ('add_log_arg', 'log_out', 'logger_thread')]:
            body = source[source.index('fn ' + method + '('):source.index('fn ' + end + '(')]
            self.assertIn('crate::guest_log::output(', body)
            self.assertIn('context.' + field + '.map(', body)
            self.assertIn('system_properties::read_bool("ro.debuggable", false)', body)
            self.assertIn('crate::guest_log::output_arg(', body)
            self.assertIn('read_file.map(', body.replace('\n            ', ''))


if __name__ == '__main__':
    unittest.main()
