"""Qualification summaries must prove that the selected live tests ran."""
import importlib.util
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
import tempfile
from unittest.mock import MagicMock, patch

TOOLS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('rehearsal_qualification', TOOLS / 'qualify_rehearsal.py')
qualification = importlib.util.module_from_spec(spec)
with patch.object(sys, 'path', [str(TOOLS), *sys.path]):
    spec.loader.exec_module(qualification)
    import qualify_history_ui


class TestQualificationDenominators(unittest.TestCase):
    def test_interactive_public_egress_is_refused_before_allocating_a_runtime(self):
        argv = ['qualify_rehearsal.py', '--engine', 'unused', '--public-egress']
        with patch.object(qualification, 'os', SimpleNamespace(name='nt')), \
                patch.object(qualification, 'windows_session_id', return_value=1), \
                patch.object(sys, 'argv', argv), patch.object(sys, 'stderr'), \
                patch.object(qualification, 'inventory') as inventory, \
                patch.object(qualification.tempfile, 'mkdtemp') as allocate, \
                patch.object(qualification, 'run') as launch:
            with self.assertRaises(SystemExit) as failure:
                qualification.main()
            self.assertEqual(failure.exception.code, 2)
            inventory.assert_not_called()
            allocate.assert_not_called()
            launch.assert_not_called()

    def test_ram_monitor_keeps_observing_after_history_fills(self):
        available = [2] + [8] * 900 + [4]
        readings = iter(available)
        kernel = MagicMock()

        def sample(status):
            status._obj.available_physical = next(readings)
            status._obj.total_physical = 16
            return True

        kernel.GlobalMemoryStatusEx.side_effect = sample
        with patch.object(qualify_history_ui.ctypes, 'WinDLL', return_value=kernel, create=True):
            monitor = qualify_history_ui.RamSamples()
        with patch.object(monitor.stop, 'wait', side_effect=[False] * (len(available) - 1) + [True]):
            monitor.thread.start()
            monitor.thread.join(timeout=2)
        report = monitor.finish()

        self.assertEqual(report['observed_count'], len(available))
        self.assertEqual(report['count'], 900)
        self.assertEqual(report['minimum_available_bytes'], available[0])
        self.assertEqual([row['available_bytes'] for row in report['samples']], available[-900:])

    def test_explicit_memory_budget_requires_admission_before_launch(self):
        with tempfile.TemporaryDirectory() as temp, \
                patch.object(qualification, 'admit_child_memory_bytes', side_effect=ValueError('too large')) as admission, \
                patch.object(qualification.subprocess, 'Popen') as launch:
            with self.assertRaises(ValueError):
                qualification.run(['fictional-child'], {}, Path(temp) / 'run.log', memory_limit_bytes=20 * 1024**3)
            admission.assert_called_once_with(20 * 1024**3)
            launch.assert_not_called()

    def test_owned_child_inherits_isolated_browser_mode_only_on_desktop(self):
        environment = {'WAYPOINT_ISOLATED_BROWSER_TESTS_ONLY': 'caller'}
        for session in [0, 1]:
            with self.subTest(session=session), tempfile.TemporaryDirectory() as temp, \
                    patch.object(qualification, 'os', SimpleNamespace(name='nt')), \
                    patch.object(qualification, 'windows_session_id', return_value=session), \
                    patch.object(qualification, 'admit_child_memory_bytes', return_value=4 * 1024**3), \
                    patch.object(qualification.subprocess, 'Popen') as launch, \
                    patch.object(qualification, 'ChildContainment'), \
                    patch.object(qualification, 'resume_owned_child'):
                child = launch.return_value
                child.poll.return_value = child.wait.return_value = 0
                result = qualification.run(['fictional-child'], environment, Path(temp) / 'run.log')
                self.assertEqual(result['exit_code'], 0)
                self.assertEqual(launch.call_args.kwargs['env']['WAYPOINT_ISOLATED_BROWSER_TESTS_ONLY'],
                                 '1' if session else 'caller')
        self.assertEqual(environment, {'WAYPOINT_ISOLATED_BROWSER_TESTS_ONLY': 'caller'})

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


class TestReplaySelection(unittest.TestCase):
    def test_frame_control_selection_refuses_mixed_denominators_before_any_runtime_work(self):
        spec = importlib.util.spec_from_file_location('replay_selection', TOOLS / 'qualify_replay.py')
        replay = importlib.util.module_from_spec(spec)
        with patch.object(sys, 'path', [str(TOOLS), *sys.path]):
            spec.loader.exec_module(replay)
        for option in ['--frame-churn', '--alias-lifecycle']:
            with self.subTest(option=option), \
                    patch.object(sys, 'argv', ['qualify_replay.py', '--engine', 'unused', '--frame-controls', option]), \
                    patch.object(replay, 'inventory') as inventory, \
                    patch.object(replay.tempfile, 'mkdtemp') as allocate, \
                    patch.object(replay, 'run') as launch, \
                    patch.object(sys, 'stderr'):
                with self.assertRaises(SystemExit) as failure:
                    replay.main()
                self.assertEqual(failure.exception.code, 2)
                inventory.assert_not_called()
                allocate.assert_not_called()
                launch.assert_not_called()


if __name__ == '__main__':
    unittest.main()
