import importlib.util
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('waypoint_coco', ROOT / 'tools/cocoindex_agent_search.py')
coco = importlib.util.module_from_spec(spec)
spec.loader.exec_module(coco)


class CocoIndexBoundaryTests(unittest.TestCase):
    def setUp(self):
        environment = mock.patch.dict(os.environ, {'WAYPOINT_COCO_CCC': ''})
        environment.start()
        self.addCleanup(environment.stop)
        (ROOT / 'target').mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix='coco-test-', dir=ROOT / 'target')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        model = mock.patch.object(coco, 'model_path', return_value=self.root / 'fictional-model')
        model.start()
        self.addCleanup(model.stop)
        self.repo = self.root / 'repo'
        self.repo.mkdir()
        subprocess.run(['git', 'init', '-q', str(self.repo)], check=True)
        (self.repo / 'source.rs').write_text('fn original() {}\n', encoding='utf-8')
        subprocess.run(['git', 'add', 'source.rs'], cwd=self.repo, check=True)
        self.base = self.root / 'cache'
        command = coco.ccc_path(coco.paths(self.base, self.repo)[0])
        command.parent.mkdir(parents=True)
        command.write_bytes(b'fixture')

    def test_refuses_secrets_and_cache_inside_checkout(self):
        (self.repo / '.env.local').write_text('fictional', encoding='utf-8')
        subprocess.run(['git', 'add', '-f', '.env.local'], cwd=self.repo, check=True)
        with self.assertRaisesRegex(ValueError, 'secret-like'):
            coco.tracked_files(self.repo)
        with mock.patch.dict(os.environ, {'WAYPOINT_COCO_HOME': str(ROOT / 'target/cache')}):
            with self.assertRaisesRegex(ValueError, 'outside'):
                coco.home()

    def test_search_requires_current_index_and_retains_receipt(self):
        def run(args, **_kwargs):
            if args[1] == 'version':
                output = coco.VERSION
            elif args[1] == 'index':
                output = 'Indexing: done, error: 0'
            else:
                output = json.dumps({'success': True, 'results': [
                    {'file_path': 'source.rs', 'start_line': 1, 'end_line': 1,
                     'score': 0.9, 'content': 'fn original() {}'}]})
            return subprocess.CompletedProcess(args, 0, stdout=output)
        with mock.patch.object(coco, '_run', side_effect=run):
            coco.index(self.base, self.repo)
            coco.search(self.base, 'find original', 5, self.repo)
            receipts = list((self.base / 'receipts').rglob('search-*.json'))
            self.assertEqual(len(receipts), 1)
            receipt = json.loads(receipts[0].read_text(encoding='utf-8'))
            self.assertEqual(receipt['result']['results'][0]['file_path'], 'source.rs')
            self.assertNotIn('find original', receipts[0].read_text(encoding='utf-8'))
            (self.repo / 'source.rs').write_text('fn changed() {}\n', encoding='utf-8')
            with self.assertRaisesRegex(RuntimeError, 'stale'):
                coco.search(self.base, 'find original', 5, self.repo)

    def test_failed_index_invalidates_previous_marker(self):
        with mock.patch.object(coco, '_run', side_effect=lambda args, **_: subprocess.CompletedProcess(args, 0, stdout=coco.VERSION if args[1] == 'version' else 'Indexing: done, error: 0')):
            coco.index(self.base, self.repo)
        marker = self.base / 'active' / (coco.paths(self.base, self.repo)[1].name + '.json')
        self.assertTrue(marker.is_file())
        def failing(args, **_kwargs):
            if args[1] == 'version':
                return subprocess.CompletedProcess(args, 0, stdout=coco.VERSION)
            raise RuntimeError('fixture failure')
        with mock.patch.object(coco, '_run', side_effect=failing):
            with self.assertRaisesRegex(RuntimeError, 'fixture failure'):
                coco.index(self.base, self.repo)
        self.assertFalse(marker.exists())

    def test_concurrent_operation_is_rejected(self):
        with coco.exclusive_lock(self.base, 'same'):
            with self.assertRaisesRegex(RuntimeError, 'another operation'):
                with coco.exclusive_lock(self.base, 'same'):
                    self.fail('lock should reject a second owner')

    def test_model_bytes_are_checked(self):
        model = self.root / 'model'
        model.mkdir()
        path = model / 'config.json'
        path.write_bytes(b'fictional')
        expected = hashlib.sha256(b'fictional').hexdigest()
        with mock.patch.object(coco, 'MODEL_HASHES', {'config.json': expected}):
            coco.verify_model(model)
            path.write_bytes(b'changed')
            with self.assertRaisesRegex(RuntimeError, 'hash mismatch'):
                coco.verify_model(model)


if __name__ == '__main__':
    unittest.main()
