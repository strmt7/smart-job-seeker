"""Explicit private-runtime downloads and fictional smoke checks in a Windows job."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import tempfile
from qualify_rehearsal import run, test_denominator
from qualify_history_ui import RamSamples

REPO = Path(__file__).resolve().parent.parent
TESTS = {
    'browser': 'private_runtime_setup_downloads_activates_and_runs_a_browser',
    'office': 'private_office_repair_downloads_activates_and_converts_docx',
    'ollama': 'private_ollama_repair_downloads_activates_and_serves_without_models',
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--component', required=True, choices=sorted(TESTS))
    parser.add_argument('--timeout', type=int, default=900)
    args = parser.parse_args()
    if os.name != 'nt' or not 60 <= args.timeout <= 1200:
        parser.error('Windows only; timeout must be 60 through 1200 seconds')
    shell = shutil.which('pwsh') or shutil.which('powershell')
    if not shell:
        parser.error('PowerShell is required for the managed build')
    sources = [Path(__file__), *[REPO / path for path in [
        'tools/qualify_rehearsal.py', 'tools/qualify_history_ui.py', 'tools/owned_process.py',
        'apps/web_app/src/runtime_setup.rs', 'apps/web_app/src/runtime_setup_qualification.rs',
        'apps/web_app/src/runtime_setup_smoke.py', 'apps/web_app/src/document_convert.rs',
        'apps/web_app/src/pdf_extract.rs', 'apps/web_app/scripts/runtime-setup.py',
        'apps/web_app/scripts/package-windows.py', 'apps/web_app/windows-runtime-artifacts.json']]]
    def hashes():
        return {p.relative_to(REPO).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}
    folder = Path(tempfile.mkdtemp(prefix='runtime-component-run-', dir=REPO / 'target'))
    log = folder / 'native.private.log'
    environment = os.environ.copy()
    environment.update(WAYPOINT_RUNTIME_SETUP_CONTAINED='1', CARGO_BUILD_JOBS='2')
    receipt = {'complete': False, 'component': args.component, 'sources': hashes(),
               'scope': 'real private runtime downloads and fictional smoke; no employer forms or model inference'}
    memory = RamSamples()
    memory.thread.start()
    try:
        receipt['run'] = run([shell, '-NoProfile', '-File', str(REPO / 'apps/web_app/scripts/build-windows.ps1'),
                              '-Test', '-TestFilter', TESTS[args.component], '-Ignored', '-NoCapture'],
                             environment, log, args.timeout)
        if receipt['run']['exit_code']:
            raise RuntimeError('Runtime qualification failed; inspect ' + str(log))
        content = log.read_text(encoding='utf-8')
        receipt['tests'] = test_denominator(content, 'rust', 1)
        matches = re.findall(r'(?:Runtime component|Private runtime setup) evidence: (.+)', content)
        if len(matches) != 1:
            raise RuntimeError('Missing or ambiguous private evidence path')
        native = Path(matches[0].strip()).resolve(strict=True)
        if not native.is_relative_to((REPO / 'target').resolve(strict=True)):
            raise RuntimeError('Qualification evidence escaped its private directory')
        report = native / 'qualification.private.json'
        value = json.loads(report.read_text(encoding='utf-8'))
        if value.get('complete') is not True:
            raise RuntimeError('Native qualification did not complete')
        receipt.update(native_report=str(report), native_report_sha256=hashlib.sha256(report.read_bytes()).hexdigest())
        if receipt['sources'] != hashes():
            raise RuntimeError('Qualification source changed while running')
        receipt['complete'] = True
    finally:
        receipt['memory'] = memory.finish()
        receipt['memory']['capture_limit_seconds'] = 450
        (folder / 'result.private.json').write_text(json.dumps(receipt, indent=2), encoding='utf-8')
        print('Private runtime qualification:', folder)


if __name__ == '__main__':
    main()
