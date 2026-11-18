"""Verify the private rehearsal launch contract without starting a model."""
from __future__ import annotations

import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

with patch.object(sys, 'path', [str(Path(__file__).resolve().parents[1]), *sys.path]):
    import qualify_private_rehearsal as launcher


@unittest.skipUnless(os.name == 'nt', 'The private rehearsal launcher targets Windows')
class PrivateRehearsalLaunchTest(unittest.TestCase):
    def test_full_journey_uses_admitted_memory_and_full_native_duration(self):
        model = 'fictional:27b'
        digest = 'b' * 64
        journey = {'sections': [[['Name', 'Avery']]], 'conditional_replans': 0,
                   'final_review': {'attachments': {}, 'untouched': ['Website']}}
        responses = {
            '/api/ps': {'models': []},
            '/api/version': {'version': 'fixture'},
        }
        target = launcher.REPO / 'target'
        target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='private-rehearsal-test-', dir=target) as directory:
            root = Path(directory)
            manifest = root / 'source-page/archive/replay.json'
            manifest.parent.mkdir(parents=True)
            manifest.write_text(json.dumps({'entry_url': 'https://example.org/fictional'}), encoding='utf-8')
            fixture = root / 'fixture.json'
            fixture.write_text(json.dumps({
                'source': {'rehearse': True, 'directory': str(root)},
                'python': str(sys.executable), 'config': {'seconds': 3600},
                'manifest_sha256': launcher.digest(manifest), 'journey': journey,
            }), encoding='utf-8')
            receipt = root / 'receipt'
            receipt.mkdir()
            choice = {'name': model, 'digest': digest, 'directory': str(root), 'size_bytes': 12 * launcher.GIB}

            def run(command, environment, log, seconds, **budget):
                payload = Path(environment['WAYPOINT_AUTO_MODEL_FIXTURE'])
                staged = json.loads(payload.read_bytes())
                self.assertEqual(staged['journey'], journey)
                self.assertEqual(staged['selection']['choice'], choice)
                self.assertEqual(environment['WAYPOINT_AUTO_MODEL_FIXTURE_SHA256'], launcher.digest(payload))
                self.assertEqual(environment['WAYPOINT_OWNED_RUNTIME_TRACE'], '1')
                self.assertEqual(budget['watched_logs'], (receipt / 'ollama-native.private.log',))
                self.assertEqual(budget['memory_limit_bytes'], 14 * launcher.GIB)
                self.assertEqual(budget['host_reserve_bytes'], 6 * launcher.GIB)
                self.assertEqual(seconds, 3900)
                self.assertEqual(command[-3:], [launcher.TEST, '-Ignored', '-NoCapture'])
                log.write_text('test result: ok. 1 passed; 0 failed;', encoding='utf-8')
                (receipt / 'automatic-rehearsal.private.json').write_text(json.dumps({
                    'model_admission': {'mode': 'private', 'session_closed': True}
                }), encoding='utf-8')
                return {'exit_code': 0}

            argv = ['qualify_private_rehearsal', '--fixture', str(fixture), '--model', model,
                    '--digest', digest, '--model-store', str(root), '--trace-native']
            with (patch.object(sys, 'argv', argv),
                  patch.object(launcher, 'local', side_effect=responses.__getitem__),
                  patch.object(launcher, 'resolve_model', return_value=choice) as resolve,
                  patch.object(launcher, 'available_physical_bytes', return_value=20 * launcher.GIB),
                  patch.object(launcher, 'RamSamples') as sampler,
                  patch.object(launcher.tempfile, 'mkdtemp', return_value=str(receipt)),
                  patch.object(launcher.shutil, 'which', return_value='pwsh'),
                  patch.object(launcher, 'run', side_effect=run)):
                sampler.return_value.finish.return_value = {}
                launcher.main()
                resolve.assert_called_once_with(root.resolve(strict=True), model)
            self.assertTrue(json.loads((receipt / 'result.private.json').read_bytes())['complete'])
