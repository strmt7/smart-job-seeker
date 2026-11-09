"""Portable checks for the worker's actual protocol and verified-copy boundary."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

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


if __name__ == '__main__':
    unittest.main()
