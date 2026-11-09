"""Run explicit headless, fictional-only Windows browser qualification.

Supply a current chrome-headless-shell-win64 directory and local Python. The
tool does not download engines, alter shared caches or enable real-site work.
All configuration, screenshots and logs stay in the ignored target directory.
"""
from __future__ import annotations

import argparse
import base64
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

from owned_process import ChildContainment, CREATE_SUSPENDED, admit_child_memory_bytes, resume_owned_child

REPO = Path(__file__).resolve().parents[1]
FIXTURE = '''<!doctype html><meta charset="utf-8"><title>Fictional rehearsal</title>
<label for="name">Applicant name</label><input id="name">
<button type="button" id="next" onclick="document.querySelector('#section').hidden=false">Continue</button>
<section id="section" hidden><label for="email">Email</label><input id="email">
<label for="resume">Fictional PDF</label><input id="resume" type="file" accept=".pdf,application/pdf"></section>'''


def fictional_pdf() -> bytes:
    """A valid blank one-page PDF; no external document package or real CV."""
    data = bytearray(b'%PDF-1.4\n%\xe2\xe3\xcf\xd3\n')
    objects = [b'<< /Type /Catalog /Pages 2 0 R >>',
               b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
               b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> /Contents 4 0 R >>',
               b'<< /Length 0 >>\nstream\nendstream']
    offsets = [0]
    for number, value in enumerate(objects, 1):
        offsets.append(len(data))
        data.extend(f'{number} 0 obj\n'.encode() + value + b'\nendobj\n')
    xref = len(data)
    data.extend(b'xref\n0 5\n0000000000 65535 f \n')
    for offset in offsets[1:]:
        data.extend(f'{offset:010d} 00000 n \n'.encode())
    data.extend(f'trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode())
    return bytes(data)


def digest(path: Path) -> str:
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def inventory(root: Path) -> dict[str, str]:
    if not root.is_absolute() or root.is_symlink() or root.is_junction() or not root.is_dir():
        raise ValueError('Engine must be an absolute directory without a reparse point')
    files = {}
    total = 0
    for path in root.rglob('*'):
        if path.is_symlink() or path.is_junction():
            raise ValueError('Engine contains a reparse point')
        if path.is_file():
            total += path.stat().st_size
            if len(files) >= 4096 or total > 600 * 1024**2:
                raise ValueError('Engine inventory exceeds the qualification budget')
            files[path.relative_to(root).as_posix()] = digest(path)
    if not {'chrome-headless-shell.exe', 'icudtl.dat'} <= files.keys():
        raise ValueError('Incomplete headless-shell engine')
    return files


def run(command: list[str], environment: dict[str, str], log: Path, seconds: int = 240) -> dict:
    if type(seconds) is not int or not 1 <= seconds <= 1200:
        raise ValueError('Command timeout must be a bounded integer')
    budget = admit_child_memory_bytes(4 * 1024**3)
    containment = None
    started = time.monotonic()
    with log.open('xb') as output:
        process = subprocess.Popen(command, cwd=REPO, env=environment, stdin=subprocess.DEVNULL,
                                   stdout=output, stderr=subprocess.STDOUT,
                                   creationflags=subprocess.CREATE_NO_WINDOW | CREATE_SUSPENDED)
        try:
            containment = ChildContainment(process, budget)
            resume_owned_child(process)
            while process.poll() is None:
                if time.monotonic() - started > seconds or log.stat().st_size > 8 * 1024**2:
                    raise RuntimeError('Qualification time or log budget exceeded')
                time.sleep(.05)
            code = process.wait(timeout=5)
        finally:
            try:
                if containment is not None:
                    containment.terminate(process)
                    containment.close()
                elif process.poll() is None:
                    process.kill()
            finally:
                process.wait(timeout=5)
    result = {'command': command, 'exit_code': code, 'seconds': round(time.monotonic() - started, 3),
              'job_memory_limit': budget, 'log_sha256': digest(log)}
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine', type=Path, required=True)
    parser.add_argument('--python', type=Path, default=Path(sys.executable))
    args = parser.parse_args()
    if os.name != 'nt':
        parser.error('This qualification requires 64-bit Windows')
    if not args.python.is_absolute() or not args.python.is_file():
        parser.error('Python must be an explicit absolute executable')
    files = inventory(args.engine)
    cargo = shutil.which('cargo')
    if not cargo:
        parser.error('Cargo is required for the Rust integration checks')
    target = REPO / 'target'
    target.mkdir(exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='rehearsal-qualification-', dir=target))
    content = folder / 'content'
    content.mkdir()
    (content / 'index.html').write_text(FIXTURE, encoding='utf-8')
    (content / 'fictional.pdf').write_bytes(fictional_pdf())
    config = {'schema': 1, 'engine_root': str(args.engine), 'engine_files': files,
              'content_root': str(content), 'content_files': {name: digest(content / name)
                  for name in ['index.html', 'fictional.pdf']},
              'entry': 'index.html', 'seconds': 90}
    path = folder / 'configuration.private.json'
    path.write_text(json.dumps(config), encoding='utf-8')
    environment = os.environ.copy()
    environment.update(WAYPOINT_REHEARSAL_CONFIG=str(path), WAYPOINT_REHEARSAL_CONFIG_SHA256=digest(path),
                       WAYPOINT_REHEARSAL_PYTHON=str(args.python), PYTHONUTF8='1', CARGO_BUILD_JOBS='2')
    sources = [REPO / 'crates/waypoint_browser' / item for item in [
        'Cargo.toml', 'src/cdp.rs', 'src/lib.rs', 'src/rehearsal.rs', 'src/rehearsal_worker.py',
        'tests/rehearsal_live.rs', 'tests/test_rehearsal_windows.py', 'tests/support/rehearsal_network.rs']]
    sources += [Path(__file__), REPO / 'Cargo.lock', REPO / 'tools/owned_process.py']
    evidence = {'schema': 1, 'source_sha256': {p.relative_to(REPO).as_posix(): digest(p) for p in sources},
                'started_at': datetime.now(timezone.utc).isoformat(),
                'config_sha256': digest(path), 'results': [], 'complete': False,
                'scope': 'fictional local forms and loopback receiver; no general no-egress claim'}
    receipt = folder / 'result.private.json'
    try:
        commands = [
            [cargo, 'test', '-p', 'waypoint_browser', '--test', 'rehearsal_live', '--', '--ignored', '--test-threads=1'],
            [str(args.python), '-X', 'utf8', '-B', str(REPO / 'crates/waypoint_browser/tests/test_rehearsal_windows.py'), '-v'],
        ]
        for index, command in enumerate(commands):
            log = folder / f'check-{index}.private.log'
            result = run(command, environment, log)
            evidence['results'].append(result)
            if result['exit_code']:
                raise RuntimeError('Qualification failed; inspect the private log: ' + str(log))
        encoded = (folder / 'driver-screenshot.base64').read_text(encoding='ascii')
        screenshot = folder / 'driver-screenshot.png'
        screenshot.write_bytes(base64.b64decode(encoded, validate=True))
        if evidence['source_sha256'] != {p.relative_to(REPO).as_posix(): digest(p) for p in sources}:
            raise RuntimeError('Qualification source changed during execution')
        evidence.update(complete=True, screenshot_sha256=digest(screenshot))
    finally:
        evidence['ended_at'] = datetime.now(timezone.utc).isoformat()
        receipt.write_text(json.dumps(evidence, indent=2), encoding='utf-8')
    print('Headless qualification passed; private evidence: ' + str(receipt))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
