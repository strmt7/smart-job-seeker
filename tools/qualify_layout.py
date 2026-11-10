"""Run headless layout checks inside an owned, memory-bounded Windows process tree."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import tempfile

from qualify_rehearsal import run
from owned_process import available_physical_bytes

REPO = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--quick', action='store_true')
    parser.add_argument('--source-assets', action='store_true')
    parser.add_argument('--browser', type=Path)
    parser.add_argument('--executable', type=Path)
    args = parser.parse_args()
    node = shutil.which('node')
    if os.name != 'nt' or not node:
        parser.error('Windows and the development Node runtime are required')
    command = [node, str(REPO / 'apps/web_app/scripts/qualify-layout.cjs')]
    for flag in ['quick', 'source_assets']:
        if getattr(args, flag):
            command.append('--' + flag.replace('_', '-'))
    for key in ['browser', 'executable']:
        selected = getattr(args, key)
        if selected:
            selected = selected.resolve(strict=True)
            if not selected.is_file():
                parser.error('Select an executable file')
            command.extend(['--' + key, str(selected)])
    folder = Path(tempfile.mkdtemp(prefix='layout-check-run-', dir=REPO / 'target'))
    sources = [REPO / name for name in ['tools/qualify_layout.py', 'tools/qualify_rehearsal.py',
        'tools/owned_process.py', 'apps/web_app/scripts/qualify-layout.cjs']]
    hashes = lambda: {p.relative_to(REPO).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}
    receipt = {'complete': False, 'started_at': datetime.now(timezone.utc).isoformat(),
               'sources': hashes(), 'available_ram_before': available_physical_bytes()}
    try:
        env = os.environ.copy()
        env['WAYPOINT_LAYOUT_CONTAINED'] = '1'
        log = folder / 'native.private.log'
        receipt['run'] = run(command, env, log, 600)
        matches = re.findall(r'^Layout evidence: (.+)$', log.read_text(encoding='utf-8'), re.MULTILINE)
        if len(matches) != 1:
            raise RuntimeError('Missing layout evidence; inspect the private log')
        native = Path(matches[0].strip()).resolve(strict=True)
        if not native.is_relative_to((REPO / 'target').resolve(strict=True)):
            raise RuntimeError('Layout evidence escaped the private target folder')
        receipt['native'] = str(native)
        result_path = native / 'results.json'
        result = json.loads(result_path.read_text(encoding='utf-8'))
        receipt['result_sha256'] = hashlib.sha256(result_path.read_bytes()).hexdigest()
        receipt['cases'] = len(result['cases'])
        if receipt['run']['exit_code'] or not result.get('completed') or result.get('error') or result['failures'] or result['scriptErrors'] or result['cleanup']['errors'] or not result['cleanup']['appExited']:
            raise RuntimeError('Layout qualification failed; retained observations and cleanup status are not a pass')
        if hashes() != receipt['sources']:
            raise RuntimeError('Layout controller source changed during qualification')
        receipt['complete'] = True
    except Exception as error:
        receipt['error'] = str(error)
        raise
    finally:
        receipt['ended_at'] = datetime.now(timezone.utc).isoformat()
        receipt['available_ram_after'] = available_physical_bytes()
        (folder / 'result.private.json').write_text(json.dumps(receipt, indent=2), encoding='utf-8')
        print('Layout qualification:', folder)


if __name__ == '__main__':
    main()
