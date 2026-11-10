"""Portable checks for the worker's actual protocol and verified-copy boundary."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

WORKER = Path(__file__).resolve().parents[1] / 'src/rehearsal_worker.py'
spec = importlib.util.spec_from_file_location('rehearsal_worker', WORKER)
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


class RehearsalProtocolTests(unittest.TestCase):
    def test_structured_commands_preserve_values_without_code_interpolation(self):
        command = {'id': 1, 'method': 'Runtime.evaluate', 'params': {
            'expression': 'fictional Unicode — 資格', 'arguments': [{'value': 'name\nsecond line'}]}}
        self.assertEqual(worker.parse_json(json.dumps(command).encode(), worker.MAX_FRAME), command)
        self.assertTrue(worker.validate_command(command))
        self.assertFalse(worker.validate_command({'op': 'close'}))

    def test_invalid_frames_and_ambiguous_keys_are_rejected(self):
        for raw in [b'{', b'{}{}', b'{"id":1,"id":2}', b'{"x":NaN}', b'\xff',
                    '{"id":1}'.encode('utf-16'), '{"id":1}'.encode('utf-32'), b'{"x":1e999}']:
            with self.subTest(raw=raw), self.assertRaises(worker.WorkerError):
                worker.parse_json(raw, worker.MAX_FRAME)
        for value in [None, [], {'op': 'close', 'extra': 1},
                      {'id': True, 'method': 'Page.enable', 'params': {}},
                      {'id': 2**53, 'method': 'Page.enable', 'params': {}},
                      {'id': 1, 'method': 'Page.enable', 'params': [], 'sessionId': 'session'},
                      {'id': 1, 'method': 'Page.enable', 'params': {}, 'sessionId': None},
                      {'id': 1, 'method': 'Runtime.evaluate', 'params': {}, 'extra': 'ignored?'}]:
            with self.subTest(value=value), self.assertRaises(worker.WorkerError):
                worker.validate_command(value)

    def test_session_configuration_requires_exact_schema_and_bounded_integer_budget(self):
        good = dict(schema=1, engine_root='fixture', engine_files={}, content_root='fixture',
                    content_files={}, entry='index.html', seconds=30)
        self.assertEqual(worker.validate_config(good), good)
        for key, value in [('schema', True), ('schema', 2), ('seconds', True),
                           ('seconds', 4), ('seconds', 3601), ('seconds', 30.0)]:
            with self.subTest(key=key, value=value), self.assertRaises(worker.WorkerError):
                worker.validate_config(dict(good, **{key: value}))
        with self.assertRaises(worker.WorkerError):
            worker.validate_config(dict(good, unknown='ignored?'))

    def test_replay_requires_explicit_manifest_and_binary_assets_remain_mode_gated(self):
        good = dict(schema=2, engine_root='fixture', engine_files={}, content_root='fixture',
                    content_files={'replay.json':'a'*64}, entry='index.html', seconds=30,
                    replay_manifest='replay.json')
        self.assertEqual(worker.validate_config(good), good)
        for changes in [{'schema':1}, {'replay_manifest':'../replay.json'},
                        {'replay_manifest':'other.json'}, {'content_files':None}]:
            with self.subTest(changes=changes), self.assertRaises(worker.WorkerError):
                worker.validate_config({**good,**changes})
        self.assertEqual(worker.validate_config({**good,'schema':3})['schema'],3)
        with self.assertRaises(worker.WorkerError):worker.validate_config({**good,'schema':4})
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);source=root/'source';source.mkdir();data=b'fictional response'
            (source/'body.bin').write_bytes(data);files={'body.bin':hashlib.sha256(data).hexdigest()}
            with self.assertRaisesRegex(worker.WorkerError, 'unsupported_snapshot_file'):
                worker.copy_verified(source,files,root/'refused',4096,True)
            worker.copy_verified(source,files,root/'replay',4096,True,True)
            self.assertEqual((root/'replay/body.bin').read_bytes(),data)

    def test_windows_paths_and_manifest_aliases_are_refused(self):
        for path in ['../file', '/root', 'C:/file', 'dir\\file', 'a//b', 'CON.pdf',
                     'dir/lpt1.txt', 'COM¹.pdf', 'LPT³.txt', 'name.', 'name ', 'file:stream', '.']:
            with self.subTest(path=path), self.assertRaises(worker.WorkerError):
                worker.relative_path(path)
        with self.assertRaises(worker.WorkerError):
            worker.manifest({'File.pdf': 'a' * 64, 'file.pdf': 'b' * 64})

    def test_copy_is_checksum_bound_and_rejects_inventory_drift(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / 'source'
            source.mkdir()
            data = b'<!doctype html><label>Fictional applicant</label>'
            (source / 'index.html').write_bytes(data)
            files = {'index.html': hashlib.sha256(data).hexdigest()}
            target = root / 'copied'
            worker.copy_verified(source, files, target, 4096, True)
            self.assertEqual((target / 'index.html').read_bytes(), data)
            (source / 'index.html').write_bytes(b'changed')
            with self.assertRaisesRegex(worker.WorkerError, 'hash_mismatch'):
                worker.copy_verified(source, files, root / 'mismatch', 4096, True)
            (source / 'extra.js').write_text('fictional extra', encoding='utf-8')
            with self.assertRaisesRegex(worker.WorkerError, 'inventory'):
                worker.copy_verified(source, files, root / 'extra', 4096, True)

    def test_executable_assets_and_copy_budget_are_refused(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / 'source'
            source.mkdir()
            data = b'fictional binary'
            (source / 'payload.exe').write_bytes(data)
            files = {'payload.exe': hashlib.sha256(data).hexdigest()}
            with self.assertRaisesRegex(worker.WorkerError, 'unsupported_snapshot_file'):
                worker.copy_verified(source, files, root / 'executable', 4096, True)
            with self.assertRaisesRegex(worker.WorkerError, 'budget'):
                worker.copy_verified(source, files, root / 'oversized', 1)


class RecoveryTests(unittest.TestCase):
    SID = 'S-1-5-21-111-222-333-1001'
    NAME = worker.PREFIX + '1' * 32

    def receipt(self):
        return {'schema': 1, 'mechanism': 'windows-job-list-v1', 'profile': self.NAME,
                'user_sid': self.SID, 'worker': {'pid': 101, 'created': '134000000000000001'},
                'browser': {'pid': 102, 'created': '134000000000000002'},
                'job': worker.job_name(self.SID, self.NAME)}

    def state(self, root, live=None):
        class API:
            user_sid = self.SID
            def identity_alive(self, identity):
                return (live or {}).get(identity['pid']) == identity['created']
        state = worker.RecoveryState.__new__(worker.RecoveryState)
        state.api, state.root = API(), root
        state.job_active = lambda _: False
        state.removed = []
        state.remove_profile = state.removed.append
        return state

    def test_ownership_and_process_stamps_require_exact_shapes(self):
        good = self.receipt()
        self.assertEqual(worker.validate_receipt(good, self.SID), good)
        for key, value in [('profile', '../outside'), ('user_sid', 'another user'), ('job', 'foreign job'),
                           ('mechanism', 'assign-after-start'), ('schema', True),
                           ('worker', {'pid': True, 'created': '123'}),
                           ('browser', {'pid': 4, 'created': '0'}),
                           ('browser', {'pid': 4, 'created': str(2**64)})]:
            with self.subTest(key=key), self.assertRaises(worker.WorkerError):
                worker.validate_receipt(dict(good, **{key: value}), self.SID)

    def test_reused_pid_does_not_authorize_touching_its_new_owner(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            state = self.state(root, {101: '134000000000009999', 102: '134000000000008888'})
            path = state.path(self.NAME)
            path.write_text(json.dumps(self.receipt()), encoding='utf-8')
            self.assertEqual(state.recover(), {'recovered': 1, 'retained': 0})
            self.assertEqual(state.removed, [self.NAME])
            self.assertFalse(path.exists())

    def test_live_original_worker_blocks_recovery_and_new_admission(self):
        with tempfile.TemporaryDirectory() as temp:
            state = self.state(Path(temp), {101: '134000000000000001'})
            path = state.path(self.NAME)
            path.write_text(json.dumps(self.receipt()), encoding='utf-8')
            with self.assertRaises(worker.RecoveryBusy):
                state.recover()
            self.assertTrue(path.exists())
            self.assertEqual(state.removed, [])

    def test_live_job_is_not_killed_when_its_owner_is_unavailable(self):
        with tempfile.TemporaryDirectory() as temp:
            state = self.state(Path(temp))
            state.job_active = lambda _: True
            path = state.path(self.NAME)
            path.write_text(json.dumps(self.receipt()), encoding='utf-8')
            ticks = iter(range(0, 100, 4))
            with patch.object(worker.time, 'monotonic', side_effect=lambda: next(ticks)):
                with self.assertRaises(worker.RecoveryBusy):
                    state.recover()
            self.assertEqual(state.removed, [])
            self.assertTrue(path.exists())

    def test_corrupt_unknown_and_mismatched_records_are_retained(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            state = self.state(root)
            state.path(self.NAME).write_text('{incomplete', encoding='utf-8')
            (root / ('2' * 32 + '.json')).write_text(json.dumps(self.receipt()), encoding='utf-8')
            (root / 'foreign.json').write_text('private unrelated data', encoding='utf-8')
            staging = root / ('3' * 32 + '.' + '4' * 32 + '.tmp')
            staging.write_bytes(b'partial uncommitted write')
            self.assertEqual(state.recover(), {'recovered': 0, 'retained': 3})
            self.assertEqual(state.removed, [])
            self.assertFalse(staging.exists())
            self.assertEqual((root / 'foreign.json').read_text(), 'private unrelated data')

    def test_directory_inventory_has_a_bounded_denominator(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for index in range(257):
                (root / str(index)).touch()
            with self.assertRaisesRegex(worker.WorkerError, 'inventory_budget'):
                worker.bounded_entries(root)

    def test_profile_storage_walk_limits_bytes_and_entries(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'cache').mkdir()
            (root / 'cache' / 'data').write_bytes(b'x' * 64)
            self.assertEqual(worker.profile_usage(root, max_entries=2, max_bytes=64), 64)
            with self.assertRaisesRegex(worker.WorkerError, 'entry_budget'):
                worker.profile_usage(root, max_entries=1)
            with self.assertRaisesRegex(worker.WorkerError, 'byte_budget'):
                worker.profile_usage(root, max_bytes=63)


if __name__ == '__main__':
    unittest.main()
