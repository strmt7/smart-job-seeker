"""Explicit headless qualification of worker shutdown and failure behavior.

Requires the same checksum-bound fictional configuration as rehearsal_live.rs.
No external host is contacted. Hard-kill tests retain the exact owned process
handle and remove only the profile returned by that worker.
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import queue
import re
import subprocess
import threading
import unittest

WORKER = Path(__file__).resolve().parents[1] / 'src/rehearsal_worker.py'
spec = importlib.util.spec_from_file_location('rehearsal_worker', WORKER)
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


class Session:
    def __init__(self, config):
        self.process = subprocess.Popen(
            [os.environ['WAYPOINT_REHEARSAL_PYTHON'], '-I', '-B', '-u', str(WORKER)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            creationflags=subprocess.CREATE_NO_WINDOW)
        self.messages = queue.Queue(16)
        self.profile = None
        self.root_handle = None
        self.api = worker.Windows()

        def read():
            try:
                while raw := self.process.stdout.readline(worker.MAX_FRAME + 2):
                    if len(raw) > worker.MAX_FRAME + 1:
                        raise ValueError('response frame too large')
                    self.messages.put(json.loads(raw), timeout=2)
            except (OSError, ValueError, queue.Full):
                self.messages.put({'type': 'test_reader_error'}, timeout=2)
            finally:
                self.messages.put({'type': 'eof'}, timeout=2)

        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()
        self.send(config)

    def send(self, value):
        self.process.stdin.write(json.dumps(value).encode('utf-8') + b'\n')
        self.process.stdin.flush()

    def receive(self):
        return self.messages.get(timeout=15)

    def ready(self):
        value = self.receive()
        if value.get('type') != 'ready':
            raise AssertionError('worker did not become ready: ' + str(value.get('code', value.get('type'))))
        name = value['profile_name']
        if not re.fullmatch(re.escape(worker.PREFIX) + '[a-f0-9]{32}', name):
            raise AssertionError('invalid worker profile identity')
        self.profile = name
        self.root_handle = self.api.OpenProcess(0x100000 | 0x1000, False, value['pid'])
        self.api.check(self.root_handle, 'test_root_handle_unavailable')
        assert value['appcontainer'] and value['capabilities'] == 0
        return value

    def finish(self):
        # The handle pins process identity even if Windows reuses the PID.
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait(timeout=5)
        if self.root_handle:
            if self.api.WaitForSingleObject(self.root_handle, 5000) != 0:
                raise AssertionError('owned browser survived worker death')
            self.api.CloseHandle(self.root_handle)
            self.root_handle = None
        self.process.stdin.close()
        self.process.stdout.close()
        self.reader.join(timeout=3)
        if self.reader.is_alive():
            raise AssertionError('reader did not stop')
        if self.profile:
            folder = Path(os.environ['LOCALAPPDATA']) / 'Packages' / self.profile / 'AC'
            if folder.exists():
                # Production recovery remains a separate open gate. The test
                # deliberately owns this profile and verifies its process ended.
                if self.api.DeleteAppContainerProfile(self.profile) < 0 or folder.exists():
                    raise AssertionError('test-owned profile recovery failed')


@unittest.skipUnless(os.name == 'nt' and os.environ.get('WAYPOINT_REHEARSAL_CONFIG'),
                     'explicit Windows headless qualification only')
class RehearsalLifecycleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        path = Path(os.environ['WAYPOINT_REHEARSAL_CONFIG']).resolve(strict=True)
        target = WORKER.parents[3] / 'target'
        assert path.is_relative_to(target.resolve(strict=True))
        raw = path.read_bytes()
        assert len(raw) <= worker.MAX_CONFIG
        assert hashlib.sha256(raw).hexdigest() == os.environ['WAYPOINT_REHEARSAL_CONFIG_SHA256']
        cls.config = worker.parse_json(raw, worker.MAX_CONFIG)

    def launch(self):
        session = Session(self.config)
        self.addCleanup(session.finish)
        session.ready()
        return session

    def assert_clean(self, session, expected_exit):
        self.assertEqual(session.process.wait(timeout=8), expected_exit)
        folder = Path(os.environ['LOCALAPPDATA']) / 'Packages' / session.profile / 'AC'
        self.assertFalse(folder.exists(), 'worker retained its profile')

    def test_stdin_eof_cleans_browser_and_profile(self):
        session = self.launch()
        session.process.stdin.close()
        self.assertEqual(session.receive()['type'], 'closed')
        self.assert_clean(session, 0)

    def test_malformed_command_fails_and_cleans_profile(self):
        session = self.launch()
        session.process.stdin.write(b'{"id":1,"id":2}\n')
        session.process.stdin.flush()
        self.assertEqual(session.receive()['code'], 'invalid_protocol_json')
        self.assert_clean(session, 1)

    def test_second_instance_is_refused_without_disturbing_first(self):
        first = self.launch()
        second = Session(self.config)
        self.addCleanup(second.finish)
        self.assertEqual(second.receive()['code'], 'another_rehearsal_is_active')
        self.assertEqual(second.process.wait(timeout=8), 1)
        first.send({'op': 'close'})
        self.assertEqual(first.receive()['type'], 'closed')
        self.assert_clean(first, 0)

    def test_worker_hard_kill_terminates_owned_browser(self):
        session = self.launch()
        session.process.kill()
        session.process.wait(timeout=5)
        self.assertEqual(session.api.WaitForSingleObject(session.root_handle, 5000), 0)
        # Session.finish performs explicit test-only profile recovery afterward.


if __name__ == '__main__':
    unittest.main()
