#!/usr/bin/env python3
"""Release checks remain active under python -O."""
import ast
import os
from pathlib import Path
import unittest
import sign_virt_apex as sva


class OptimizedTest(unittest.TestCase):
    def test_no_release_decision_uses_assert(self):
        tree = ast.parse(Path(sva.__file__).read_text())
        self.assertFalse([node.lineno for node in ast.walk(tree) if isinstance(node, ast.Assert)])

    def test_failed_command_is_always_an_exception(self):
        class Args:
            verbose = False
            avbtool = 'avbtool'
        with self.assertRaises(ValueError):
            sva.RunCommand(Args(), ['/bin/sh', '-c', 'exit 19'])


if __name__ == '__main__':
    unittest.main()
