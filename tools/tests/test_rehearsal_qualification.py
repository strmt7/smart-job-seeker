"""Qualification summaries must prove that the selected live tests ran."""
import importlib.util
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

TOOLS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('rehearsal_qualification', TOOLS / 'qualify_rehearsal.py')
qualification = importlib.util.module_from_spec(spec)
with patch.object(sys, 'path', [str(TOOLS), *sys.path]):
    spec.loader.exec_module(qualification)


class TestQualificationDenominators(unittest.TestCase):
    def test_exact_rust_and_python_runs_are_counted(self):
        self.assertEqual(qualification.test_denominator(
            'test result: \x1b[32mok\x1b[0m. 2 passed; 0 failed; 0 ignored; 0 measured;', 'rust', 2),
            {'executed_tests': 2, 'ignored_tests': 0})
        self.assertEqual(qualification.test_denominator('Ran 6 tests in 1.0s\n\nOK\n', 'python', 6),
                         {'executed_tests': 6, 'ignored_tests': 0})

    def test_zero_ignored_failed_or_ambiguous_runs_are_refused(self):
        for summary in [
            'test result: ok. 0 passed; 0 failed; 0 ignored;',
            'test result: ok. 1 passed; 0 failed; 1 ignored;',
            'test result: ok. 2 passed; 1 failed; 0 ignored;',
            'test result: ok. 2 passed; 0 failed; 0 ignored;\n' * 2,
            'compilation succeeded',
        ]:
            with self.subTest(summary=summary), self.assertRaises(RuntimeError):
                qualification.test_denominator(summary, 'rust', 2)
        for summary in ['Ran 6 tests in 1.0s\nOK (skipped=1)\n', 'Ran 0 tests in 0.0s\nOK\n', 'OK']:
            with self.subTest(summary=summary), self.assertRaises(RuntimeError):
                qualification.test_denominator(summary, 'python', 6)


if __name__ == '__main__':
    unittest.main()
