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
from unittest.mock import MagicMock

TOOLS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('owned_answers', TOOLS / 'qualify_owned_answers.py')
qualification = importlib.util.module_from_spec(spec)
with patch.object(sys, 'path', [str(TOOLS), *sys.path]): spec.loader.exec_module(qualification)


class OwnedAnswerQualification(unittest.TestCase):
    def test_metadata_timeout_retries_the_same_live_server_without_restarting(self):
        server=MagicMock();server.poll.return_value=None
        errors=[];config={'version':'fixture','model':'fictional:tiny','digest':'a'*64}
        with patch.object(qualification,'ensure_host_reserve'), patch.object(qualification.time,'sleep'), \
                patch.object(qualification,'local_json',side_effect=[TimeoutError(),{'version':'fixture'},{'models':[{'name':'fictional:tiny','digest':'a'*64}]}]) as query:
            self.assertEqual(qualification.wait_runtime_ready(server,'http://127.0.0.1:49999',config,errors),2)
        self.assertEqual(query.call_count,3);self.assertEqual(errors,['TimeoutError'])
        server.kill.assert_not_called()

    def test_readiness_refuses_expired_observation_budget_and_wrong_runtime(self):
        server=MagicMock();server.poll.return_value=None
        config={'version':'fixture','model':'fictional:tiny','digest':'a'*64}
        with patch.object(qualification,'ensure_host_reserve'), patch.object(qualification.time,'monotonic',side_effect=[0,61]), self.assertRaises(RuntimeError):
            qualification.wait_runtime_ready(server,'http://127.0.0.1:49999',config,[])
        with patch.object(qualification,'ensure_host_reserve'), patch.object(qualification,'local_json',return_value={'version':'different'}), self.assertRaises(RuntimeError):
            qualification.wait_runtime_ready(server,'http://127.0.0.1:49999',config,[])

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
            self.assertEqual(env['LLAMA_ARG_CACHE_RAM'], '512')
            self.assertEqual(env['LLAMA_ARG_CTX_CHECKPOINTS'], '1')
            for key in ['USERPROFILE','APPDATA','LOCALAPPDATA','TEMP','TMP']:
                self.assertTrue(Path(env[key]).is_relative_to(folder))
        self.assertEqual(dict(os.environ), before)

    def test_unbounded_inherited_prompt_caches_are_scoped_to_owned_children(self):
        with tempfile.TemporaryDirectory() as temp, patch.dict(os.environ, {
            'LLAMA_ARG_CACHE_RAM':'-1', 'LLAMA_ARG_CTX_CHECKPOINTS':'1000'
        }):
            folder=Path(temp)
            env=qualification.server_environment(folder,folder/'models',49151)
            self.assertEqual(env['LLAMA_ARG_CACHE_RAM'],'512')
            self.assertEqual(env['LLAMA_ARG_CTX_CHECKPOINTS'],'1')
            self.assertEqual(os.environ['LLAMA_ARG_CACHE_RAM'],'-1')
            self.assertEqual(os.environ['LLAMA_ARG_CTX_CHECKPOINTS'],'1000')

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


class RecordedFrameVisionInput(unittest.TestCase):
    PIXEL = b'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/ZmQAAAAASUVORK5CYII='

    def fixture(self, folder):
        cases = []
        for name, (file, count, status) in qualification.FRAME_VISION_CASES.items():
            (folder / file).write_bytes(self.PIXEL)
            cases.append({'id':name, 'image_file':file, 'image_base64_sha256':hashlib.sha256(self.PIXEL).hexdigest(),
                          'expected':[[f'Fictional field {i}',f'Value {i}'] for i in range(count)], 'expected_status':status})
        native = json.dumps({'complete':True,'profile_removed':True,'vision_cases':cases}).encode()
        (folder / 'native.private.json').write_bytes(native)
        path = folder / 'result.private.json'
        path.write_text(json.dumps({'complete':True,'frame_controls':True,'source_engine_unchanged':True,
                                    'tests':{'executed_tests':1,'ignored_tests':0},'native_sha256':hashlib.sha256(native).hexdigest()}),encoding='utf-8')
        return path

    def test_verified_images_are_bound_to_native_controls_and_denominator(self):
        target = qualification.REPO / 'target'; target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='replay-qualification-',dir=target) as temp:
            path = self.fixture(Path(temp))
            payload = qualification.recorded_frame_vision(path)
            self.assertEqual(payload['schema'],1)
            self.assertEqual(sum(len(case['expected']) for case in payload['cases']),6)
            self.assertTrue(all(case['image'].encode()==self.PIXEL for case in payload['cases']))
            self.assertEqual(payload['replay_result_sha256'],qualification.digest(path))

    def test_changed_or_partial_evidence_is_refused_before_model_staging(self):
        target = qualification.REPO / 'target'; target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='replay-qualification-',dir=target) as temp:
            folder = Path(temp)
            for variant, pattern in [('image','image digest'),('native','verified native'),('partial','verified native')]:
                with self.subTest(variant=variant):
                    path = self.fixture(folder)
                    if variant=='image': (folder/'controls-screenshot.base64').write_bytes(self.PIXEL+b'A')
                    elif variant=='native': (folder/'native.private.json').write_bytes(b'{}')
                    else:
                        report = json.loads(path.read_text(encoding='utf-8')); report['complete']=False
                        path.write_text(json.dumps(report),encoding='utf-8')
                    with self.assertRaisesRegex(ValueError,pattern): qualification.recorded_frame_vision(path)

    def test_unrelated_or_path_traversing_inputs_do_not_stage(self):
        target = qualification.REPO / 'target'; target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='replay-qualification-',dir=target) as temp:
            folder = Path(temp); path = self.fixture(folder)
            native_path = folder/'native.private.json'
            native = json.loads(native_path.read_text(encoding='utf-8')); native['vision_cases'][0]['image_file']='../unrelated.base64'
            native_path.write_text(json.dumps(native),encoding='utf-8')
            report = json.loads(path.read_text(encoding='utf-8')); report['native_sha256']=qualification.digest(native_path)
            path.write_text(json.dumps(report),encoding='utf-8')
            with self.assertRaisesRegex(ValueError,'Invalid recorded vision criteria'): qualification.recorded_frame_vision(path)
        with tempfile.TemporaryDirectory() as temp:
            path = self.fixture(Path(temp))
            with self.assertRaisesRegex(ValueError,'directly under target'): qualification.recorded_frame_vision(path)

    def test_system_requires_distinct_label_bound_focus_images(self):
        target = qualification.REPO / 'target'; target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='replay-qualification-',dir=target) as temp:
            folder = Path(temp); path = self.fixture(folder)
            with self.assertRaisesRegex(ValueError,'lack separate focused captures'):
                qualification.recorded_frame_vision(path,require_focus=True)
            native_path = folder/'native.private.json'
            native = json.loads(native_path.read_text(encoding='utf-8')); controls=native['vision_cases'][0]
            controls['focused_images']=[{'label':pair[0],'image_file':f'control-focus-{i}.base64','image_base64_sha256':controls['image_base64_sha256']} for i,pair in enumerate(controls['expected'])]
            native_path.write_text(json.dumps(native),encoding='utf-8')
            report = json.loads(path.read_text(encoding='utf-8')); report['native_sha256']=qualification.digest(native_path)
            path.write_text(json.dumps(report),encoding='utf-8')
            with self.assertRaisesRegex(ValueError,'repeated or mismatched'):
                qualification.recorded_frame_vision(path,require_focus=True)


if __name__ == '__main__': unittest.main()
