"""Qualification summaries must prove that the selected live tests ran."""
import importlib.util
from pathlib import Path
import sys
import unittest
import tempfile
from unittest.mock import MagicMock, patch

TOOLS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('rehearsal_qualification', TOOLS / 'qualify_rehearsal.py')
qualification = importlib.util.module_from_spec(spec)
with patch.object(sys, 'path', [str(TOOLS), *sys.path]):
    spec.loader.exec_module(qualification)


class TestQualificationDenominators(unittest.TestCase):
    def test_live_reserve_refuses_low_or_unavailable_memory(self):
        with patch.object(qualification, 'available_physical_bytes', return_value=3 * 1024**3):
            qualification.ensure_host_reserve()
        with patch.object(qualification, 'available_physical_bytes', return_value=3 * 1024**3 - 1):
            with self.assertRaisesRegex(RuntimeError, 'host RAM reserve'):
                qualification.ensure_host_reserve()
        with patch.object(qualification, 'available_physical_bytes', side_effect=OSError('unknown')):
            with self.assertRaises(OSError):
                qualification.ensure_host_reserve()

    def test_pressure_during_run_closes_only_the_owned_containment(self):
        child = MagicMock()
        child.poll.return_value = None
        containment = MagicMock()
        containment.terminate.side_effect = lambda process: setattr(process.poll, 'return_value', 1)
        with tempfile.TemporaryDirectory() as temp, \
                patch.object(qualification, 'admit_child_memory_bytes', return_value=4 * 1024**3), \
                patch.object(qualification.subprocess, 'Popen', return_value=child), \
                patch.object(qualification, 'ChildContainment', return_value=containment), \
                patch.object(qualification, 'resume_owned_child'), \
                patch.object(qualification, 'ensure_host_reserve', side_effect=RuntimeError('pressure')):
            with self.assertRaisesRegex(RuntimeError, 'pressure'):
                qualification.run(['fictional-child'], {}, Path(temp) / 'run.log')
        containment.terminate.assert_called_once_with(child)
        containment.close.assert_called_once()
        child.wait.assert_called_once_with(timeout=5)
        child.kill.assert_not_called()

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

    def test_control_cleanup_removes_only_the_owned_execution_directory(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = Path(temp)
            control = folder / 'control-engine'
            control.mkdir()
            (control / 'fixture.bin').write_bytes(b'fictional runtime')
            source = folder / 'source.bin'
            source.write_bytes(b'preserve source')
            qualification.cleanup_control_runtime(folder)
            self.assertFalse(control.exists())
            self.assertEqual(source.read_bytes(), b'preserve source')

    def test_cleanup_failure_cannot_leave_a_successful_receipt(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = Path(temp)
            evidence = {'complete': True}
            with patch.object(qualification, 'cleanup_control_runtime', side_effect=RuntimeError('fixture')):
                with self.assertRaises(RuntimeError): qualification.finish_evidence(evidence, folder)
            self.assertFalse(evidence['complete'])
            self.assertFalse(evidence['control_cleanup_complete'])
            self.assertTrue((folder / 'result.private.json').is_file())


if __name__ == '__main__':
    unittest.main()
