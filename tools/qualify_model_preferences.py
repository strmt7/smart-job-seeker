"""Headless compiled settings, private rehearsal and cancellation qualification.

Use only a previously verified fictional source copy under target. The caller
provides an existing isolated control folder with managed browser/PDF runtimes.
No employer interaction, external model API or shared-model mutation is used.
"""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from urllib.error import HTTPError
from urllib.request import Request, ProxyHandler, build_opener

from owned_process import ChildContainment, CREATE_SUSPENDED, admit_child_memory_bytes, resume_owned_child
from qualify_history_ui import RamSamples
from qualify_rehearsal import ensure_host_reserve

REPO = Path(__file__).resolve().parents[1]


def main():
    from playwright.sync_api import sync_playwright, expect
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--control', type=Path, required=True)
    parser.add_argument('--model', required=True)
    parser.add_argument('--model-store', type=Path, required=True)
    parser.add_argument('--corpus', type=Path, help='Private frozen form-corpus metadata')
    parser.add_argument('--case-id', help='Case ID in --corpus')
    parser.add_argument('--ui-only', action='store_true', help='Repeat only settings interactions after a UI change')
    args = parser.parse_args()
    if bool(args.corpus) != bool(args.case_id): parser.error('--corpus and --case-id are required together')
    if args.ui_only and args.corpus: parser.error('Settings-only checks cannot score form cases')
    if os.name != 'nt': parser.error('Windows native qualification only')
    fixture = args.fixture.resolve(strict=True)
    control = args.control.resolve(strict=True)
    source_data = json.loads(fixture.read_bytes())
    source = Path(source_data['source']['directory']).resolve(strict=True)
    assert source_data['source']['rehearse'] is True
    for path in (fixture, control, source):
        if not path.is_relative_to(REPO / 'target') or path.is_symlink() or path.is_junction():
            parser.error('Fictional fixtures and isolated controls must stay under target')
    model_store = args.model_store.resolve(strict=True)
    engine_id = json.loads((control / 'tool-browser.json').read_bytes())['id']
    assert re.fullmatch('[0-9a-f]{32}', engine_id)
    engine = control / 'runtime-tools' / engine_id / 'app/dependencies/chromium/chrome-headless-shell.exe'
    app = REPO / 'target/debug/waypoint_web.exe'
    folder = Path(tempfile.mkdtemp(prefix='model-settings-native-', dir=REPO / 'target'))
    workspace = folder / 'workspace'
    workspace.mkdir()
    client = build_opener(ProxyHandler({}))

    def request(url, value=None, token=None):
        headers = {'Content-Type': 'application/json'}
        if token: headers['X-Workspace-Token'] = token
        try:
            with client.open(Request(url, data=None if value is None else json.dumps(value).encode(), headers=headers), timeout=10) as response:
                data = response.read(10 * 1024**2 + 1)
        except HTTPError as error:
            detail = error.read(4096).decode('utf-8', errors='replace')
            with (folder / 'http-errors.private.log').open('a', encoding='utf-8') as log:
                log.write(str(error.code) + ' ' + url.removeprefix(locals().get('origin', '')) + ' ' + detail + '\n')
            raise
        assert len(data) <= 10 * 1024**2
        return json.loads(data)

    assert request('http://127.0.0.1:11434/api/ps')['models'] == []
    report = {'complete': False, 'scope': 'Compiled app, fictional saved form, installed local vision model; no live employer interaction',
              'app_sha256': hashlib.sha256(app.read_bytes()).hexdigest()}
    sources = [Path(__file__).resolve(), REPO / 'Cargo.lock', REPO / 'tools/form_corpus.py', REPO / 'apps/web_app/data/rehearsal-profile.json', REPO / 'crates/waypoint_policy/data/source_permissions.json',
               *sorted((REPO / 'crates/waypoint_browser/src').glob('*.*')),
               *sorted((REPO / 'crates/waypoint_applications/src').glob('*.rs')), REPO / 'crates/waypoint_inference/src/ollama.rs', *sorted((REPO / 'apps/web_app/src').glob('*.*')),
               *sorted((REPO / 'apps/web_app/web').glob('*.*'))]
    source_hashes = lambda: {str(path.relative_to(REPO)): hashlib.sha256(path.read_bytes()).hexdigest() for path in sources}
    report['sources'] = source_hashes()
    from form_corpus import bind_case, evaluate_case
    profile = REPO / 'apps/web_app/data/rehearsal-profile.json'
    manifest = source / 'source-page/archive/replay.json'
    binding = bind_case(args.corpus, args.case_id, manifest, profile) if args.corpus else None
    report['evaluation'] = {'status': 'unclassified', 'scored': False} if binding is None else {'status': 'pending', 'binding': binding}
    process = owner = browser = None
    memory = RamSamples()
    memory.thread.start()
    environment = {**os.environ, 'APPDATA': str(control.parent), 'WAYPOINT_WORKSPACE_DIR': str(workspace),
                   'WAYPOINT_NO_BROWSER': '1', 'WAYPOINT_NO_TRAY': '1'}
    try:
        log = folder / 'app.private.log'
        with log.open('xb') as output:
            process = subprocess.Popen([str(app)], env=environment, stdin=subprocess.DEVNULL, stdout=output,
                stderr=subprocess.STDOUT, creationflags=subprocess.CREATE_NO_WINDOW | CREATE_SUSPENDED)
            owner = ChildContainment(process, admit_child_memory_bytes((4 if args.ui_only else 10) * 1024**3))
            resume_owned_child(process)
            deadline = time.monotonic() + 25
            while not (address := re.search(rb'Smart Jobseeker: (http://127\.0\.0\.1:[0-9]+)/#([0-9a-f]{64})', log.read_bytes())):
                ensure_host_reserve()
                if process.poll() is not None or time.monotonic() > deadline: raise RuntimeError('App startup failed')
                time.sleep(.1)
            origin, token = (value.decode() for value in address.groups())
            api = lambda route, value=None: request(origin + route, value, token)
            entry = json.loads((source / 'source-page/archive/replay.json').read_bytes())['entry_url']
            job = api('/api/jobs/add', {'url': entry, 'title': 'Fictional three-section rehearsal', 'employer': 'Fictional organization',
                'description': 'Fictional application controls for private-model qualification.'})['id']
            attempt = source_data['source']['attempt_id']
            record = workspace / 'application-records' / (hashlib.sha256(job.encode()).hexdigest()[:16] + '-' + attempt)
            shutil.copytree(source, record)
            for name in ('attempt.json', 'source-page-summary.json'):
                path = record / name
                value = json.loads(path.read_bytes())
                value['job_id'] = job
                path.write_text(json.dumps(value), encoding='utf-8')

            def poll(task, seconds=400):
                end = time.monotonic() + seconds
                while True:
                    ensure_host_reserve()
                    value = api('/api/job-source/status', {'task_id': task})
                    if value['status'] != 'running': return value
                    if time.monotonic() >= end: raise RuntimeError('Rehearsal did not stop')
                    time.sleep(.3)

            with sync_playwright() as pw:
                browser = pw.chromium.launch(headless=True, executable_path=str(engine), args=['--disable-gpu'])
                context = browser.new_context(viewport={'width': 1360, 'height': 940})
                context.route('**/*', lambda route: route.continue_() if route.request.url.startswith(origin + '/') else route.abort())
                page = context.new_page()
                errors = []
                page.on('pageerror', lambda error: errors.append(str(error)))
                page.goto(origin + '/#' + token, wait_until='domcontentloaded')
                page.locator('nav[aria-label="Workspace sections"] a[href="#settings-section"]').click()
                page.locator('#local-tools').evaluate('node => node.open = true')
                page.locator('#model-inventory').click()
                row = page.locator('#model-inventory-info li').filter(has_text=args.model)
                expect(row).to_have_count(1)
                settings = page.locator('#model-preferences')
                settings.locator('input').fill(str(model_store))
                initial = api('/api/model-preferences')
                row.get_by_role('button', name='Use for rehearsal', exact=True).click()
                expect(settings.get_by_role('status')).to_contain_text('Rehearsal model: ' + args.model, timeout=10000)
                saved = api('/api/model-preferences')
                assert saved['revision'] == initial['revision'] + 1 and saved['choice']['name'] == args.model
                try:
                    api('/api/model-preferences', {'revision': initial['revision'], 'model': None, 'directory': None})
                    raise AssertionError('Stale setting overwrote the selection')
                except HTTPError as error:
                    report['stale_write_status'] = error.code
                assert api('/api/model-preferences') == saved
                page.reload(wait_until='domcontentloaded')
                page.locator('nav[aria-label="Workspace sections"] a[href="#settings-section"]').click()
                page.locator('#local-tools').evaluate('node => node.open = true')
                expect(settings.get_by_role('status')).to_contain_text(args.model)
                measures = []
                for width in (1360, 480, 320):
                    page.set_viewport_size({'width': width, 'height': 940})
                    settings.scroll_into_view_if_needed()
                    assert not page.evaluate('document.documentElement.scrollWidth > innerWidth')
                    box = settings.bounding_box()
                    assert box and box['width'] <= width
                    settings.screenshot(path=str(folder / f'model-settings-{width}.png'), animations='disabled')
                    refresh = settings.get_by_role('button', name='Refresh selection', exact=True)
                    refresh.scroll_into_view_if_needed()
                    refresh.click()
                    expect(refresh).to_be_enabled()
                    actions = settings.locator('.source-copy-actions')
                    actions.evaluate("node => node.scrollIntoView({block:'center'})")
                    actions.screenshot(path=str(folder / f'model-actions-{width}.png'), animations='disabled')
                    measures.append({'width': width, 'panel_width': box['width']})
                report['settings_geometry'] = measures
                settings.get_by_role('button', name='Clear selection', exact=True).click()
                expect(settings.get_by_role('status')).to_contain_text('No rehearsal model selected.')
                cleared = api('/api/model-preferences')
                assert cleared['choice'] is None and cleared['revision'] == saved['revision'] + 1
                page.locator('#model-inventory').click()
                settings.locator('input').fill(str(model_store))
                row.get_by_role('button', name='Use for rehearsal', exact=True).click()
                expect(settings.get_by_role('status')).to_contain_text(args.model)
                saved = api('/api/model-preferences')
                assert saved['revision'] == cleared['revision'] + 1
                assert not errors, errors
                browser.close()
                browser = None

            if args.ui_only:
                assert report['sources'] == source_hashes()
                report.update(complete=True, scope='Compiled settings interactions only; model lifecycle not repeated')
                return

            normal = api('/api/job-source/preview-start', {'id': job, 'attempt_id': attempt, 'rehearse': True})['task_id']
            outcome = poll(normal)
            (folder / 'rehearsal.private.json').write_text(json.dumps(outcome, indent=2), encoding='utf-8')
            assert outcome['status'] == 'complete' and outcome['record_saved'], outcome.get('error', outcome['status'])
            preview = outcome['result']['preview']
            assert preview['model_admission']['session_closed'] is True
            if binding is not None:
                if bind_case(args.corpus, args.case_id, manifest, profile) != binding:
                    raise RuntimeError('Frozen corpus inputs changed during qualification')
                evidence_root = workspace / 'application-records' / (hashlib.sha256(job.encode()).hexdigest()[:16] + '-' + outcome['attempt_id']) / 'screenshots'
                report['evaluation'] = evaluate_case(binding, preview['rehearsal'], preview['model_admission'], evidence_root=evidence_root)
            else:
                assert len(preview['rehearsal']['fields']) == 3
                assert all(item['status'] == 'visually_matched' for item in preview['rehearsal']['fields'])
            history = api('/api/attempts/list', {'id': job})
            assert any(item['attempt_id'] == normal for item in history['attempts'])

            cancel = api('/api/job-source/preview-start', {'id': job, 'attempt_id': attempt, 'rehearse': True})['task_id']
            end = time.monotonic() + 180
            while True:
                paths = list((workspace / 'application-records').glob('*-' + cancel + '/**/model-session.json'))
                if len(paths) == 1:
                    session = json.loads(paths[0].read_bytes())
                    if session.get('session_state') == 'ready': break
                ensure_host_reserve()
                if time.monotonic() >= end: raise RuntimeError('Private model did not reach ready before cancellation')
                time.sleep(.1)
            api('/api/job-source/cancel', {'task_id': cancel})
            cancelled = poll(cancel, 30)
            (folder / 'cancelled.private.json').write_text(json.dumps(cancelled, indent=2), encoding='utf-8')
            assert cancelled['status'] != 'complete', cancelled
            spec = importlib.util.spec_from_file_location('local_model_worker', REPO / 'apps/web_app/src/local_model_worker.py')
            worker = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(worker)
            exited = worker.process_state({'pid': session['pid'], 'created': session['creation_token']})
            endpoint = worker.listener({'port': session['port']})
            assert exited['exited'] and endpoint['owners'] == []
            cancel_root = paths[0].parent
            assert not (cancel_root / 'model-runtime').exists()
            assert not list(cancel_root.glob('local-model-runtime-*'))
            assert request('http://127.0.0.1:11434/api/ps')['models'] == []
            state = api('/api/state')
            assert next(item['job']['state'] for item in state['jobs'] if item['job']['id'] == job) == 'discovered'
            cleared = api('/api/model-preferences', {'revision': saved['revision'], 'model': None, 'directory': None})
            assert cleared['choice'] is None and cleared['revision'] == saved['revision'] + 1
            if binding is not None and bind_case(args.corpus, args.case_id, manifest, profile) != binding:
                raise RuntimeError('Frozen corpus inputs changed during qualification')
            assert report['sources'] == source_hashes(), 'Qualification source changed during execution'
            report.update(job_id=job, rehearsal_task=normal, cancel_task=cancel,
                          cancellation_exit=exited, cancellation_endpoint=endpoint, shared_models_unchanged=True)
            if binding is not None and not report['evaluation']['field_gate_passed']:
                raise AssertionError('Form field gate was not met; inspect the recorded denominators')
            report['complete'] = True
    finally:
        try:
            if browser is not None: browser.close()
            if process is not None:
                if owner is not None:
                    owner.terminate(process)
                    owner.close()
                elif process.poll() is None: process.kill()
                process.wait(timeout=5)
            report['process_tree_drained'] = True
        finally:
            report['memory'] = memory.finish()
            (folder / 'result.private.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
            print('Private model-settings qualification:', folder, flush=True)


if __name__ == '__main__':
    main()
