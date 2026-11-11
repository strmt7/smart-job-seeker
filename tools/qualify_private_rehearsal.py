"""Run the production private-model rehearsal against a hash-bound fictional copy.

No shared model is loaded, unloaded or replaced. Native worker/job ownership
contains the real local model, browser and test process; no live form is used.
"""
from __future__ import annotations
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
from urllib.request import ProxyHandler, Request, build_opener

from qualify_rehearsal import run
from qualify_history_ui import RamSamples

REPO = Path(__file__).resolve().parents[1]
TEST = 'recorded_preview::tests::automatic_private_model_rehearsal'


def local(path):
    with build_opener(ProxyHandler({})).open('http://127.0.0.1:11434' + path, timeout=3) as response:
        data = response.read(1024**2 + 1)
    if len(data) > 1024**2:
        raise ValueError('Local model metadata exceeded its bounds')
    return json.loads(data)


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--model', required=True)
    parser.add_argument('--digest', required=True)
    parser.add_argument('--model-store', type=Path, required=True)
    parser.add_argument('--cache-root', type=Path)
    args = parser.parse_args()
    if os.name != 'nt': parser.error('Windows native qualification only')
    fixture = args.fixture.resolve(strict=True)
    if not fixture.is_relative_to(REPO / 'target') or fixture.stat().st_size > 128 * 1024:
        parser.error('Use a bounded fictional fixture under target')
    value = json.loads(fixture.read_bytes())
    if not value['source']['rehearse']: parser.error('Fictional saved-copy rehearsal is required')
    source = Path(value['source']['directory']).resolve(strict=True)
    if not source.is_relative_to(REPO / 'target'): parser.error('Source copy must remain private under target')
    model_store = args.model_store.resolve(strict=True)
    if not model_store.is_dir() or model_store.is_symlink() or model_store.is_junction(): parser.error('Select a regular local model store')
    if local('/api/ps').get('models') != []: parser.error('A shared model is active; no test was started')
    tags = local('/api/tags')
    models = [item for item in tags.get('models', []) if item.get('name') == args.model]
    if len(models) != 1 or models[0].get('digest') != args.digest or models[0].get('details', {}).get('format') != 'gguf':
        parser.error('Installed local model does not match the requested manifest')
    folder = Path(tempfile.mkdtemp(prefix='private-rehearsal-qualification-', dir=REPO / 'target'))
    cache = args.cache_root.resolve(strict=True) if args.cache_root else folder / 'model-cache'
    if not cache.is_relative_to(REPO / 'target') or cache.is_symlink() or cache.is_junction():
        parser.error('Qualification caches must stay under target without reparse points')
    url = json.loads((source / 'source-page/archive/replay.json').read_bytes())['entry_url']
    payload = {key: value[key] for key in ['source', 'python', 'config', 'manifest_sha256']}
    payload.update(url=url, selection={'choice': {'name': args.model, 'digest': args.digest,
        'size_bytes': models[0]['size'], 'directory': str(model_store)}, 'cache_root': str(cache)})
    input_path = folder / 'fixture.private.json'
    input_path.write_text(json.dumps(payload), encoding='utf-8')
    paths = [REPO / name for name in ['apps/web_app/src/recorded_preview.rs', 'apps/web_app/src/private_inference.rs',
        'apps/web_app/src/local_model_worker.py', 'apps/web_app/src/owned_runtime.rs',
        'apps/web_app/src/runtime_start.rs', 'apps/web_app/src/model_start.rs', 'apps/web_app/src/model_preferences.rs',
        'apps/web_app/src/visual_review.rs', 'apps/web_app/src/doc_suggest.rs', 'apps/web_app/src/job_capture.rs',
        'apps/web_app/src/main.rs', 'crates/waypoint_browser/src/lib.rs', 'crates/waypoint_browser/src/captured_frame.rs',
        'crates/waypoint_browser/src/rehearsal.rs', 'crates/waypoint_browser/src/rehearsal_worker.py',
        'crates/waypoint_browser/src/rehearsal_replay.py', 'Cargo.lock']]
    paths.extend([Path(__file__).resolve(), REPO / 'tools/owned_process.py', REPO / 'tools/qualify_rehearsal.py'])
    hashes = lambda: {str(path.relative_to(REPO)): digest(path) for path in paths}
    report = {'complete': False, 'started_at': datetime.now(timezone.utc).isoformat(), 'scope': 'Fictional saved archive with production automatic private-model admission',
        'model': args.model, 'digest': args.digest, 'sources': hashes(), 'fixture_sha256': digest(input_path),
        'source_fixture_sha256': digest(fixture), 'shared_version': local('/api/version')}
    memory = RamSamples()
    memory.thread.start()
    try:
        shell = shutil.which('pwsh') or shutil.which('powershell')
        if not shell: raise RuntimeError('Windows PowerShell is unavailable')
        environment = {**os.environ, 'WAYPOINT_AUTO_MODEL_FIXTURE': str(input_path),
            'WAYPOINT_AUTO_MODEL_FIXTURE_SHA256': report['fixture_sha256'],
            'CARGO_BUILD_JOBS': '2', 'RUST_TEST_THREADS': '1', 'CARGO_TERM_COLOR': 'never'}
        report['run'] = run([shell, '-NoProfile', '-NonInteractive', '-File', str(REPO / 'apps/web_app/scripts/build-windows.ps1'),
            '-Test', '-TestFilter', TEST, '-Ignored', '-NoCapture'], environment, folder / 'native.private.log', 1000,
            memory_limit_bytes=10 * 1024**3)
        if report['run']['exit_code']:
            raise RuntimeError('Automatic private-model qualification failed; inspect ' + str(folder))
        if report['sources'] != hashes(): raise RuntimeError('Qualification source changed during execution')
        if local('/api/ps').get('models') != []: raise RuntimeError('Shared model state changed during qualification')
        result_path = folder / 'automatic-rehearsal.private.json'
        result = json.loads(result_path.read_bytes())
        if result['model_admission']['mode'] != 'private' or result['model_admission']['session_closed'] is not True:
            raise RuntimeError('Private runtime lifecycle was not confirmed')
        report['native_result_sha256'] = digest(result_path)
        report['complete'] = True
    finally:
        report['memory'] = memory.finish()
        report['ended_at'] = datetime.now(timezone.utc).isoformat()
        report['cache_retained'] = cache.exists()
        (folder / 'result.private.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print('Private rehearsal qualification:', folder)


if __name__ == '__main__':
    main()
