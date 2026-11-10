"""Private model staging must not touch other weights, profiles or services."""
import hashlib
import importlib.util
import json
import os
import socket
import shutil
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

TOOLS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('owned_answers', TOOLS / 'qualify_owned_answers.py')
qualification = importlib.util.module_from_spec(spec)
with patch.object(sys, 'path', [str(TOOLS), *sys.path]): spec.loader.exec_module(qualification)


class OwnedAnswerQualification(unittest.TestCase):
    def test_cleanup_removes_only_owned_disposable_copies_and_keeps_reports(self):
        with tempfile.TemporaryDirectory(prefix='owned-answer-qualification-', dir=qualification.REPO / 'target') as temp:
            folder = Path(temp)
            for name in ['models','runtime-profile']:
                (folder / name).mkdir(); (folder / name / 'fictional').write_bytes(b'owned temporary bytes')
            (folder / 'result.private.json').write_text('{}')
            qualification.cleanup_copies(folder)
            self.assertFalse((folder / 'models').exists()); self.assertFalse((folder / 'runtime-profile').exists())
            self.assertTrue((folder / 'result.private.json').exists())
        with tempfile.TemporaryDirectory() as temp, self.assertRaises(RuntimeError):
            qualification.cleanup_copies(Path(temp))

    @unittest.skipUnless(os.name == 'nt', 'Windows native TCP table required')
    def test_native_endpoint_owner_matches_a_real_loopback_socket(self):
        with socket.socket() as listener:
            listener.bind(('127.0.0.1',0)); listener.listen(1)
            port = listener.getsockname()[1]
            self.assertEqual(qualification.listener_owner(port)['pid'], os.getpid())
            identity = qualification.listener_identity(shutil.which('pwsh') or shutil.which('powershell'), port, os.getpid())
            self.assertTrue(identity['created']); self.assertTrue(identity['executable'])
            with self.assertRaises(RuntimeError): qualification.listener_identity('', port, os.getpid()+1)

    def stage(self, root):
        store = root / 'source'; (store / 'blobs').mkdir(parents=True)
        blob = b'fictional model bytes'
        checksum = hashlib.sha256(blob).hexdigest()
        (store / 'blobs' / ('sha256-' + checksum)).write_bytes(blob)
        (store / 'blobs' / 'unreferenced').write_bytes(b'preserve unrelated model')
        relative = Path('manifests/registry.ollama.ai/library/fictional/tiny')
        (store / relative).parent.mkdir(parents=True)
        raw = json.dumps({'config':{'digest':'sha256:' + checksum,'size':len(blob)},'layers':[]}).encode()
        (store / relative).write_bytes(raw)
        return store, hashlib.sha256(raw).hexdigest(), checksum

    def test_only_manifest_referenced_verified_bytes_are_copied(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); store, expected, checksum = self.stage(root)
            files, total = qualification.model_inventory(store, 'fictional:tiny', expected)
            with patch.object(qualification, 'ensure_host_reserve'):
                qualification.copy_model(store, root / 'copy', files)
            self.assertGreater(total, 0)
            self.assertEqual((root / 'copy/blobs' / ('sha256-' + checksum)).read_bytes(), b'fictional model bytes')
            self.assertFalse((root / 'copy/blobs/unreferenced').exists())
            self.assertEqual((store / 'blobs/unreferenced').read_bytes(), b'preserve unrelated model')

    def test_changed_blob_or_manifest_is_refused(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); store, expected, checksum = self.stage(root)
            with self.assertRaises(ValueError): qualification.model_inventory(store, 'fictional:tiny', '0' * 64)
            files, _ = qualification.model_inventory(store, 'fictional:tiny', expected)
            (store / 'blobs' / ('sha256-' + checksum)).write_bytes(b'different model bytes')
            with patch.object(qualification, 'ensure_host_reserve'), self.assertRaises(ValueError):
                qualification.copy_model(store, root / 'copy', files)

    def test_v2_active_manifest_takes_precedence_over_a_legacy_downgrade_anchor(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); store, expected, _ = self.stage(root)
            legacy = store / 'manifests/registry.ollama.ai/library/fictional/tiny'
            v2 = store / 'manifests-v2/ollama.com/library/fictional/tiny'
            v2.parent.mkdir(parents=True); v2.write_bytes(legacy.read_bytes())
            legacy.write_bytes(b'fictional retained downgrade anchor')
            files, _ = qualification.model_inventory(store, 'fictional:tiny', expected)
            self.assertIn(v2.relative_to(store), files)
            self.assertNotIn(legacy.relative_to(store), files)
            v2.write_bytes(b'changed active manifest')
            with self.assertRaises(ValueError): qualification.model_inventory(store, 'fictional:tiny', expected)

    def test_environment_is_private_and_does_not_mutate_the_host(self):
        before = dict(os.environ)
        with tempfile.TemporaryDirectory() as temp:
            folder = Path(temp)
            env = qualification.server_environment(folder, folder / 'models', 49151)
            self.assertEqual(env['OLLAMA_HOST'], '127.0.0.1:49151')
            self.assertEqual(env['OLLAMA_NO_CLOUD'], '1')
            self.assertEqual(env['OLLAMA_NUM_PARALLEL'], '1')
            self.assertEqual(env['OLLAMA_MAX_LOADED_MODELS'], '1')
            self.assertEqual(env['OLLAMA_NOPRUNE'], '1')
            for key in ['USERPROFILE','APPDATA','LOCALAPPDATA','TEMP','TMP']:
                self.assertTrue(Path(env[key]).is_relative_to(folder))
        self.assertEqual(dict(os.environ), before)

    def test_unknown_shared_residency_never_passes(self):
        for value in [{}, {'models':None}, {'models':''}, {'models':{}}]:
            with patch.object(qualification, 'local_json', return_value=value), self.assertRaises(RuntimeError):
                qualification.resident_models('http://127.0.0.1:11434')

    def test_worker_without_controller_nonce_cannot_launch(self):
        with tempfile.TemporaryDirectory(dir=qualification.REPO / 'target') as temp:
            path = Path(temp) / 'config.json'; path.write_text('{}')
            with patch.object(qualification.subprocess, 'Popen') as launch, self.assertRaises(RuntimeError):
                qualification.worker(path)
            launch.assert_not_called()


if __name__ == '__main__': unittest.main()
