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
OBSERVATIONS = []
OUTPUT = None


BOOTSTRAP = '''import json,runpy,sys,time
ns=runpy.run_path(sys.argv[1],run_name='fictional_recovery_fixture')
config=ns['validate_config'](ns['parse_json'](sys.stdin.buffer.readline(ns['MAX_CONFIG']+1),ns['MAX_CONFIG']))
api=ns['Windows'](); owner=ns['Container'](api); checkpoint=sys.argv[2]
def pause():
    if owner.process.pid:
        owned=ns['wt'].BOOL()
        api.check(api.IsProcessInJob(owner.process.process,owner.job,ns['ct'].byref(owned)),'test_job_inspection_failed')
        assert owned.value
        api.token(owner.process.process,owner.sid)
    ns['emit']({'type':'checkpoint','profile_name':owner.name,'pid':owner.process.pid,'checkpoint':checkpoint})
    while True: time.sleep(.02)
if checkpoint=='before_profile':
    api.CreateAppContainerProfile=lambda *args: pause()
elif checkpoint=='after_profile':
    create=api.CreateAppContainerProfile
    def interrupted(*args):
        result=create(*args)
        if result<0: raise RuntimeError('test profile creation failed')
        pause()
    api.CreateAppContainerProfile=interrupted
elif checkpoint=='after_atomic_create':
    create=api.CreateProcessW
    def interrupted(*args):
        result=create(*args)
        api.check(result,'test process creation failed')
        pause()
    api.CreateProcessW=interrupted
else: raise RuntimeError('unknown test checkpoint')
try:
    binary,content,sid=owner.prepare(config)
    owner.launch(binary,sid)
finally: owner.close()
'''


def recover():
    result = subprocess.run(
        [os.environ['WAYPOINT_REHEARSAL_PYTHON'], '-I', '-B', '-u', str(WORKER), '--recover'],
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        timeout=20, check=False, creationflags=subprocess.CREATE_NO_WINDOW)
    assert len(result.stdout) <= 8192
    return result.returncode, json.loads(result.stdout)


class Session:
    def __init__(self, config, checkpoint=None):
        args = [os.environ['WAYPOINT_REHEARSAL_PYTHON'], '-I', '-B', '-u']
        args += ['-c', BOOTSTRAP, str(WORKER), checkpoint] if checkpoint else [str(WORKER)]
        self.process = subprocess.Popen(
            args,
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
        value = self.messages.get(timeout=15)
        if value.get('type') == 'closed':
            peak = value.get('peak_job_commit_bytes')
            minimum = value.get('minimum_sampled_available_ram_bytes')
            storage = value.get('maximum_sampled_profile_bytes')
            assert type(peak) is int and 0 < peak <= 2 * 1024**3
            assert type(minimum) is int and minimum > 0
            assert type(storage) is int and 0 < storage <= 1024**3
            OBSERVATIONS.append({'peak_job_commit_bytes': peak,
                'minimum_sampled_available_ram_bytes': minimum,
                'maximum_sampled_profile_bytes': storage,
                'audited_processes': value['audited_processes']})
        return value

    def ready(self, expected='ready'):
        value = self.receive()
        if value.get('type') != expected:
            raise AssertionError('worker did not become ready: ' + str(value.get('code', value.get('type'))))
        name = value['profile_name']
        if not re.fullmatch(re.escape(worker.PREFIX) + '[a-f0-9]{32}', name):
            raise AssertionError('invalid worker profile identity')
        self.profile = name
        if value['pid']:
            self.root_handle = self.api.OpenProcess(0x100000 | 0x1000, False, value['pid'])
            self.api.check(self.root_handle, 'test_root_handle_unavailable')
        if expected == 'ready':
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
                code, report = recover()
                if code or report.get('type') != 'recovery' or folder.exists():
                    raise AssertionError('automatic owned-profile recovery failed')


@unittest.skipUnless(os.name == 'nt' and os.environ.get('WAYPOINT_REHEARSAL_CONFIG'),
                     'explicit Windows headless qualification only')
class RehearsalLifecycleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        global OUTPUT
        path = Path(os.environ['WAYPOINT_REHEARSAL_CONFIG']).resolve(strict=True)
        target = WORKER.parents[3] / 'target'
        assert path.is_relative_to(target.resolve(strict=True))
        raw = path.read_bytes()
        assert len(raw) <= worker.MAX_CONFIG
        assert hashlib.sha256(raw).hexdigest() == os.environ['WAYPOINT_REHEARSAL_CONFIG_SHA256']
        cls.config = worker.parse_json(raw, worker.MAX_CONFIG)
        OUTPUT = path.parent / 'worker-resources.private.json'

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
        # Keep a non-owning handle open so the kernel must return
        # WAIT_ABANDONED on the next acquisition, rather than destroy/recreate
        # the named mutex when the worker dies.
        holding = session.api.CreateMutexW(None, False,
            'Global\\SmartJobseeker.RehearsalAdmission.' + session.api.user_sid)
        session.api.check(holding, 'test_admission_handle_unavailable')
        self.addCleanup(session.api.CloseHandle, holding)
        session.process.kill()
        session.process.wait(timeout=5)
        self.assertEqual(session.api.WaitForSingleObject(session.root_handle, 5000), 0)
        folder = Path(os.environ['LOCALAPPDATA']) / 'Packages' / session.profile / 'AC'
        self.assertTrue(folder.exists(), 'hard-kill recovery baseline is missing')
        next_session = self.launch()
        self.assertFalse(folder.exists(), 'next-session recovery retained the old profile')
        next_session.send({'op': 'close'})
        self.assertEqual(next_session.receive()['type'], 'closed')
        self.assert_clean(next_session, 0)

    def test_recovery_does_not_interrupt_an_active_session(self):
        session = self.launch()
        code, report = recover()
        self.assertEqual(code, 1)
        self.assertEqual(report.get('code'), 'another_rehearsal_is_active')
        self.assertEqual(session.api.WaitForSingleObject(session.root_handle, 0), 258)
        session.send({'op': 'close'})
        self.assertEqual(session.receive()['type'], 'closed')
        self.assert_clean(session, 0)

    def test_interrupted_startup_intent_profile_and_atomic_process_are_recovered(self):
        for checkpoint in ['before_profile', 'after_profile', 'after_atomic_create']:
            with self.subTest(checkpoint=checkpoint):
                session = Session(self.config, checkpoint)
                self.addCleanup(session.finish)
                evidence = session.ready('checkpoint')
                record = Path(os.environ['LOCALAPPDATA']) / 'SmartJobseeker/rehearsal-state-v1' / (worker.profile_id(session.profile) + '.json')
                receipt = worker.validate_receipt(json.loads(record.read_text(encoding='utf-8')), session.api.user_sid)
                self.assertEqual(receipt['worker']['pid'], session.process.pid)
                self.assertIsNone(receipt['browser'], 'checkpoint must precede saving the browser stamp')
                self.assertEqual(bool(evidence['pid']), checkpoint == 'after_atomic_create')
                session.process.kill()
                session.process.wait(timeout=5)
                if session.root_handle:
                    self.assertEqual(session.api.WaitForSingleObject(session.root_handle, 5000), 0)
                code, report = recover()
                self.assertEqual(code, 0)
                self.assertGreaterEqual(report.get('recovered', 0), 1)
                self.assertFalse(record.exists())
                self.assertFalse((Path(os.environ['LOCALAPPDATA']) / 'Packages' / session.profile / 'AC').exists())


if __name__ == '__main__':
    try:
        unittest.main()
    finally:
        if OUTPUT is not None:
            OUTPUT.write_text(json.dumps({'schema': 1,
                'scope': 'normal worker closes only; interrupted processes are not measured',
                'closed_sessions': OBSERVATIONS}, indent=2), encoding='utf-8')
