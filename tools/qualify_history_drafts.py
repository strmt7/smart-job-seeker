"""Run history-backed fictional CV/letter drafting through a gated local model."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import tempfile
from qualify_history_ui import RamSamples
from qualify_rehearsal import run, test_denominator

REPO = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--model', default='gemma4:12b-it-q4_K_M')
    args = parser.parse_args()
    shell = shutil.which('pwsh') or shutil.which('powershell')
    if os.name != 'nt' or not shell:
        parser.error('Windows and PowerShell are required')
    folder = Path(tempfile.mkdtemp(prefix='history-document-run-', dir=REPO / 'target'))
    paths = [Path(__file__), *[REPO / path for path in [
        'apps/web_app/src/main.rs', 'apps/web_app/src/draft_history.rs',
        'apps/web_app/src/draft_history_qualification.rs', 'apps/web_app/src/draft_task.rs',
        'apps/web_app/src/candidate_history.rs', 'apps/web_app/src/resident_inference.rs',
        'apps/web_app/src/model_start.rs', 'apps/web_app/src/gpu_resources.rs',
        'apps/web_app/src/experience_inference.rs', 'tools/qualify_history_ui.py',
        'crates/waypoint_inference/src/ollama.rs', 'crates/waypoint_inference/src/document_draft.schema.json', 'tools/qualify_rehearsal.py',
        'tools/owned_process.py']]]
    hashes = lambda: {path.relative_to(REPO).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}
    receipt = {'complete': False, 'scope': 'Fictional checked history to CV and cover-letter drafts; no browser or external form', 'sources': hashes(), 'model_requested': args.model}
    memory = RamSamples()
    memory.thread.start()
    try:
        env = os.environ.copy()
        env.update(WAYPOINT_HISTORY_DRAFT_CONTAINED='1', WAYPOINT_VISION_BENCHMARK_MODEL=args.model, CARGO_BUILD_JOBS='1', PYTHONUTF8='1')
        log = folder / 'native.private.log'
        receipt['run'] = run([shell, '-NoProfile', '-File', str(REPO / 'apps/web_app/scripts/build-windows.ps1'), '-Test', '-TestFilter', 'resource_gated_history_backed_document_drafts', '-Ignored', '-NoCapture'], env, log, 600)
        text = log.read_text(encoding='utf-8')
        matches = re.findall(r'History document evidence: (.+)', text)
        if len(matches) != 1:
            raise RuntimeError('Missing or ambiguous native evidence directory')
        native = Path(matches[0].strip()).resolve(strict=True)
        if not native.is_relative_to((REPO / 'target').resolve(strict=True)):
            raise RuntimeError('Evidence escaped its private directory')
        result = native / 'results.private.json'
        receipt['native'] = str(native)
        if result.is_file():
            receipt['result_sha256'] = hashlib.sha256(result.read_bytes()).hexdigest()
        if receipt['run']['exit_code']:
            raise RuntimeError('Local-model qualification failed; inspect ' + str(log))
        receipt['tests'] = test_denominator(text, 'rust', 1)
        if hashes() != receipt['sources']:
            raise RuntimeError('Selected qualification source changed during the run')
        receipt['complete'] = True
    finally:
        receipt['memory'] = memory.finish()
        (folder / 'result.private.json').write_text(json.dumps(receipt, indent=2), encoding='utf-8')
        print('History document qualification:', folder)


if __name__ == '__main__':
    main()
