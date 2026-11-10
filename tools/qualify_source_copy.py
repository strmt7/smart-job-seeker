"""Qualify compiled source-copy UI in a contained, disposable Windows workspace.

Copies selected test-owned managed runtimes; does not install into the host or
use a personal browser profile. Only the native worker reads a public RFC page.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import tempfile
from qualify_rehearsal import run
from qualify_history_ui import RamSamples

REPO = Path(__file__).resolve().parent.parent


def copy_runtime(source: Path, destination: Path):
    if not source.is_dir() or source.is_symlink() or source.is_junction():
        raise ValueError('Runtime source must be a regular private directory')
    total = 0
    count = 0
    for path in source.rglob('*'):
        if path.is_symlink() or path.is_junction():
            raise ValueError('Runtime source contains a reparse point')
        if path.is_file():
            total += path.stat().st_size
            count += 1
        elif not path.is_dir():
            raise ValueError('Unexpected runtime file type')
        if total > 1200 * 1024**2 or count > 10000:
            raise ValueError('Runtime copy budget exceeded')
    shutil.copytree(source, destination)
    return {'files': count, 'bytes': total}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runtime-control', type=Path, required=True)
    parser.add_argument('--browser', type=Path, required=True)
    args = parser.parse_args()
    if os.name != 'nt' or not args.browser.is_absolute() or not args.browser.is_file():
        parser.error('Windows and an explicit headless-capable Chromium executable are required')
    source = args.runtime_control.resolve(strict=True)
    target = (REPO / 'target').resolve(strict=True)
    if not source.is_relative_to(target) or source.is_symlink() or source.is_junction():
        parser.error('Only a test-owned runtime folder inside target can be copied')
    node = shutil.which('node')
    if not node:
        parser.error('Node is needed for this development qualification')
    folder = Path(tempfile.mkdtemp(prefix='source-copy-ui-', dir=target))
    control = folder / 'appdata' / 'smart-job-seeker-control'
    control.mkdir(parents=True)
    receipt = {'complete': False, 'runtimes': {}, 'sources': {}}
    paths = [Path(__file__), REPO / 'apps/web_app/scripts/qualify-source-copy.cjs',
             REPO / 'target/debug/waypoint_web.exe', REPO / 'apps/web_app/src/job_capture.rs',
             REPO / 'apps/web_app/web/source-copy.js', REPO / 'apps/web_app/web/workspace-theme.css',
             REPO / 'apps/web_app/web/app.js', REPO / 'apps/web_app/web/index.html',
             REPO / 'apps/web_app/src/main.rs', REPO / 'apps/web_app/src/attempt_record.rs',
             REPO / 'apps/web_app/src/runtime_setup.rs', REPO / 'tools/qualify_rehearsal.py',
             REPO / 'tools/owned_process.py']
    hashes = lambda: {str(p.relative_to(REPO)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
    receipt['sources'] = hashes()
    memory = RamSamples()
    memory.thread.start()
    try:
        for pointer, relative in [('tool-browser.json', 'runtime-tools'), ('pdf-runtime.json', 'pdf-runtimes')]:
            data = (source / pointer).read_bytes()
            if len(data) > 256:
                raise ValueError('Runtime pointer exceeds its budget')
            value = json.loads(data)
            if not re.fullmatch('[a-fA-F0-9]{32}', value.get('id', '')):
                raise ValueError('Invalid runtime identity')
            if pointer == 'pdf-runtime.json' and value.get('kind') != 'portable':
                raise ValueError('Qualification requires a portable private Python')
            destination = control / relative / value['id']
            receipt['runtimes'][pointer] = copy_runtime(source / relative / value['id'], destination)
            (control / pointer).write_bytes(data)
        environment = os.environ.copy()
        environment.update(WAYPOINT_SOURCE_QA_FOLDER=str(folder), WAYPOINT_SOURCE_QA_BROWSER=str(args.browser), PYTHONUTF8='1')
        receipt['run'] = run([node, str(REPO / 'apps/web_app/scripts/qualify-source-copy.cjs')], environment, folder / 'native.private.log', 300)
        receipt['rendered'] = json.loads((folder / 'rendered.private.json').read_text(encoding='utf-8'))
        if receipt['run']['exit_code'] or receipt['rendered'].get('complete') is not True:
            raise RuntimeError('Source-copy qualification failed; inspect private evidence in ' + str(folder))
        if hashes() != receipt['sources']:
            raise RuntimeError('Qualification source changed during the run')
        receipt['complete'] = True
    finally:
        receipt['memory'] = memory.finish()
        (folder / 'result.private.json').write_text(json.dumps(receipt, indent=2), encoding='utf-8')
        print('Source-copy qualification:', folder)


if __name__ == '__main__':
    main()
