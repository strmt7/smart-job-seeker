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
import re
import runpy
import shutil
import tempfile
from urllib.request import ProxyHandler, build_opener

from owned_process import available_physical_bytes
from qualify_rehearsal import run
from qualify_history_ui import RamSamples

REPO = Path(__file__).resolve().parents[1]
TEST = 'recorded_preview::tests::automatic_private_model_rehearsal'
GIB = 1024**3


def local(path):
    with build_opener(ProxyHandler({})).open('http://127.0.0.1:11434' + path, timeout=3) as response:
        data = response.read(1024**2 + 1)
    if len(data) > 1024**2:
        raise ValueError('Local model metadata exceeded its bounds')
    return json.loads(data)


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def resolve_model(store, name):
    worker = runpy.run_path(str(REPO / 'apps/web_app/src/local_model_worker.py'))
    return worker['resolve_choice']({'model': name, 'directory': str(store)})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--model', required=True)
    parser.add_argument('--digest', required=True)
    parser.add_argument('--model-store', type=Path, required=True)
    parser.add_argument('--cache-root', type=Path)
    parser.add_argument('--job-memory-gib', type=int,
                        help='Override the aggregate test-job limit, subject to host admission')
    parser.add_argument('--host-reserve-gib', type=int, default=6,
                        help='Keep this much physical memory available during the test (minimum 3)')
    parser.add_argument('--timeout-seconds', type=int,
                        help='Override the outer timeout for setup, rehearsal and cleanup')
    parser.add_argument('--trace-native', action='store_true',
                        help='Keep native responses and runtime diagnostics privately under target')
    args = parser.parse_args()
    if os.name != 'nt': parser.error('Windows native qualification only')
    if args.host_reserve_gib < 3 or (args.job_memory_gib is not None and args.job_memory_gib < 1):
        parser.error('Set a positive job allocation and a host reserve of at least 3 GiB')
    if args.timeout_seconds is not None and args.timeout_seconds < 1:
        parser.error('The outer timeout must be a positive number of seconds')
    fixture = args.fixture.resolve(strict=True)
    if not fixture.is_relative_to(REPO / 'target') or fixture.stat().st_size > 128 * 1024:
        parser.error('Use a bounded fictional fixture under target')
    value = json.loads(fixture.read_bytes())
    if not value['source']['rehearse']: parser.error('Fictional saved-copy rehearsal is required')
    seconds = value['config']['seconds']
    if type(seconds) is not int or not 5 <= seconds <= 3600:
        parser.error('Invalid saved rehearsal duration')
    timeout = args.timeout_seconds or max(1000, seconds + 300)
    reserve_bytes = args.host_reserve_gib * GIB
    available_budget = available_physical_bytes() - reserve_bytes
    job_budget = min(16 * GIB, available_budget) if args.job_memory_gib is None else args.job_memory_gib * GIB
    if not 0 < job_budget <= available_budget:
        parser.error('Available physical RAM cannot accommodate the requested job and host reserve')
    source = Path(value['source']['directory']).resolve(strict=True)
    if not source.is_relative_to(REPO / 'target'): parser.error('Source copy must remain private under target')
    model_store = args.model_store.resolve(strict=True)
    if not model_store.is_dir() or model_store.is_symlink() or model_store.is_junction(): parser.error('Select a regular local model store')
    if local('/api/ps').get('models') != []: parser.error('A shared model is active; no test was started')
    try:
        choice = resolve_model(model_store, args.model)
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.error(f'Could not resolve the selected local model from its store: {error}')
    if choice['digest'] != args.digest:
        parser.error('Installed local model does not match the requested manifest')
    folder = Path(tempfile.mkdtemp(prefix='private-rehearsal-qualification-', dir=REPO / 'target'))
    cache = args.cache_root.resolve(strict=True) if args.cache_root else folder / 'model-cache'
    if not cache.is_relative_to(REPO / 'target') or cache.is_symlink() or cache.is_junction():
        parser.error('Qualification caches must stay under target without reparse points')
    url = json.loads((source / 'source-page/archive/replay.json').read_bytes())['entry_url']
    payload = {key: value[key] for key in ['source', 'python', 'config', 'manifest_sha256']}
    for key in ('journey', 'checked_history'):
        if key in value:
            payload[key] = value[key]
    payload.update(url=url, selection={'choice': choice, 'cache_root': str(cache)})
    input_path = folder / 'fixture.private.json'
    input_path.write_text(json.dumps(payload), encoding='utf-8')
    paths = [REPO / name for name in ['apps/web_app/src/recorded_preview.rs', 'apps/web_app/src/private_inference.rs',
        'apps/web_app/src/local_model_worker.py', 'apps/web_app/src/owned_runtime.rs',
        'apps/web_app/src/runtime_start.rs', 'apps/web_app/src/model_start.rs', 'apps/web_app/src/model_preferences.rs',
        'apps/web_app/src/visual_review.rs', 'apps/web_app/src/recorded_summary.rs', 'apps/web_app/src/section_planning.rs',
        'apps/web_app/src/native_month.rs', 'apps/web_app/src/rehearsal_candidate.rs',
        'apps/web_app/src/candidate_history.rs', 'apps/web_app/src/history_advice.rs',
        'apps/web_app/src/history_batch.rs', 'apps/web_app/src/history_record_choice.rs',
        'apps/web_app/src/rehearsal_facts.rs', 'apps/web_app/src/doc_suggest.rs', 'apps/web_app/src/job_capture.rs',
        'apps/web_app/src/main.rs', 'crates/waypoint_browser/src/lib.rs', 'crates/waypoint_browser/src/captured_frame.rs',
        'crates/waypoint_browser/src/frame_identity.rs', 'crates/waypoint_browser/src/file_readback.rs',
        'crates/waypoint_browser/src/field_label_context.js', 'crates/waypoint_browser/src/recorded_section_control.js',
        'crates/waypoint_browser/src/rehearsal.rs', 'crates/waypoint_browser/src/recorded_summary.rs',
        'crates/waypoint_browser/src/recorded_summary.js', 'crates/waypoint_browser/src/recorded_summary_viewport.js',
        'crates/waypoint_browser/src/rehearsal_worker.py',
        'crates/waypoint_browser/src/rehearsal_replay.py', 'Cargo.lock']]
    paths.extend([Path(__file__).resolve(), REPO / 'tools/owned_process.py', REPO / 'tools/qualify_rehearsal.py'])
    hashes = lambda: {str(path.relative_to(REPO)): digest(path) for path in paths}
    report = {'complete': False, 'started_at': datetime.now(timezone.utc).isoformat(), 'scope': 'Fictional saved archive with production automatic private-model admission',
        'model': args.model, 'digest': args.digest, 'sources': hashes(), 'fixture_sha256': digest(input_path),
        'source_fixture_sha256': digest(fixture), 'shared_version': local('/api/version'),
        'host_reserve_bytes': reserve_bytes, 'timeout_seconds': timeout}
    memory = RamSamples()
    memory.thread.start()
    try:
        shell = shutil.which('pwsh') or shutil.which('powershell')
        if not shell: raise RuntimeError('Windows PowerShell is unavailable')
        environment = {**os.environ, 'WAYPOINT_AUTO_MODEL_FIXTURE': str(input_path),
            'WAYPOINT_AUTO_MODEL_FIXTURE_SHA256': report['fixture_sha256'],
            'CARGO_BUILD_JOBS': '2', 'RUST_TEST_THREADS': '1', 'CARGO_TERM_COLOR': 'never'}
        environment.pop('WAYPOINT_OWNED_VISION_REPORT', None)
        environment.pop('WAYPOINT_OWNED_RUNTIME_TRACE', None)
        watched_logs = ()
        if args.trace_native:
            environment.update(WAYPOINT_OWNED_VISION_REPORT=str(folder / 'native-vision.private.json'),
                               WAYPOINT_OWNED_RUNTIME_TRACE='1')
            watched_logs = (folder / 'ollama-native.private.log',)
        log = folder / 'native.private.log'
        report['run'] = run([shell, '-NoProfile', '-NonInteractive', '-File', str(REPO / 'apps/web_app/scripts/build-windows.ps1'),
            '-Test', '-TestFilter', TEST, '-Ignored', '-NoCapture'], environment, log, timeout,
            memory_limit_bytes=job_budget, host_reserve_bytes=reserve_bytes, watched_logs=watched_logs)
        if report['run']['exit_code'] or not re.search(
                r'test result: ok\. 1 passed;', log.read_text(encoding='utf-8', errors='replace')):
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
