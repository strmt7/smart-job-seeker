"""Actual fictional profile/template-to-PDF UI journey with gated local inference."""
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
    parser.add_argument('--control', type=Path, required=True, help='Test-owned private Office/PDF runtime directory in target')
    parser.add_argument('--browser', type=Path)
    parser.add_argument('--model', default='gemma4:12b-it-q4_K_M')
    parser.add_argument('--flow', choices=['documents','template','template-letter','signature'], default='documents')
    args = parser.parse_args()
    control = args.control.resolve(strict=True)
    if os.name != 'nt' or not control.is_relative_to((REPO / 'target').resolve(strict=True)):
        parser.error('Windows and an explicit test-owned target runtime are required')
    node, shell = shutil.which('node'), shutil.which('pwsh') or shutil.which('powershell')
    if not shell or (args.flow != 'signature' and (not node or not args.browser or not args.browser.is_absolute() or not args.browser.is_file())):
        parser.error('Node, PowerShell and an explicit Chromium executable are required')
    paths = {}
    for filename, relative, binary in [('tool-office.json','runtime-tools','app/dependencies/libreoffice/program/soffice.com'),('pdf-runtime.json','pdf-runtimes','python.exe')]:
        value = json.loads((control / filename).read_text(encoding='utf-8'))
        if not re.fullmatch('[a-fA-F0-9]{32}',value.get('id','')):
            raise ValueError('Invalid managed runtime identity')
        paths[filename] = (control / relative / value['id'] / binary).resolve(strict=True)
    folder = Path(tempfile.mkdtemp(prefix='document-journey-run-', dir=REPO / 'target'))
    sources = [Path(__file__), *[REPO / path for path in [
        'apps/web_app/src/main.rs','apps/web_app/src/document_ui_qualification.rs',
        'apps/web_app/scripts/qualify-document-journey.cjs','apps/web_app/src/draft_task.rs',
        'apps/web_app/src/draft_history.rs','apps/web_app/src/draft_identity.rs','apps/web_app/src/document_visual.rs',
        'apps/web_app/src/document_visual.schema.json','apps/web_app/src/template_plan.rs',
        'apps/web_app/src/template_suggest.rs','apps/web_app/src/answer_task.rs','apps/web_app/src/fit_task.rs','apps/web_app/src/doc_suggest.rs','apps/web_app/src/docx_edit.rs','apps/web_app/src/docx_edit_task.rs',
        'crates/waypoint_inference/src/template_edit_plan.schema.json','apps/web_app/scripts/qualify-template-journey.cjs',
        'apps/web_app/src/letter_export.rs','apps/web_app/src/document_convert.rs',
        'apps/web_app/src/history_record_choice.rs','apps/web_app/src/candidate_history.rs',
        'apps/web_app/src/pdf_extract.rs','apps/web_app/src/pdf_extract.py','apps/web_app/src/pdf_render.py','apps/web_app/src/pdf_text_geometry.py',
        'apps/web_app/src/pdf_visual_guard.py','apps/web_app/src/pdf_job.rs',
        'apps/web_app/src/document_extract.rs','apps/web_app/src/attempt_record.rs',
        'apps/web_app/src/document_job_check.rs','apps/web_app/src/ats_check.rs',
        'apps/web_app/src/resident_inference.rs','apps/web_app/src/experience_inference.rs',
        'apps/web_app/src/model_start.rs','apps/web_app/src/gpu_resources.rs','apps/web_app/src/visual_review.rs',
        'tools/qualify_history_ui.py',
        'apps/web_app/web/app.js','apps/web_app/web/workspace-theme.css',
        'crates/waypoint_inference/src/ollama.rs','crates/waypoint_inference/src/document_draft.schema.json',
        'tools/qualify_rehearsal.py','tools/owned_process.py']]]
    hashes = lambda: {p.relative_to(REPO).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}
    receipt = {'complete':False,'sources':hashes(),'model':None if args.flow == 'signature' else args.model,'flow':args.flow,'scope':'Fixed fictional bodies, profile signatures, canonical text, PDF export/pagination and rendered geometry; no inference or forms' if args.flow == 'signature' else 'Actual fictional profile/template, drafting, PDF export, text/visual checks and stale-source refusal; no employer forms'}
    selected = [paths['pdf-runtime.json'],paths['tool-office.json']]
    if args.flow != 'signature': selected.extend([args.browser,control/'fictional.docx'])
    receipt['selected_files']={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in selected}
    memory = RamSamples(); memory.thread.start()
    try:
        env = os.environ.copy()
        env.update(WAYPOINT_DOCUMENT_UI_FLOW=args.flow,APPDATA=str(folder/'appdata'),WAYPOINT_DOCUMENT_UI_CONTAINED='1',WAYPOINT_DOCUMENT_UI_NODE=node,
            WAYPOINT_DOCUMENT_UI_BROWSER=str(args.browser or ''),WAYPOINT_DOCUMENT_UI_TEMPLATE=str(control/'fictional.docx'),
            WAYPOINT_PDF_PYTHON=str(paths['pdf-runtime.json']),WAYPOINT_SOFFICE=str(paths['tool-office.json']),
            WAYPOINT_VISION_BENCHMARK_MODEL=args.model,CARGO_BUILD_JOBS='1',PYTHONUTF8='1')
        log = folder/'native.private.log'
        test = 'contained_profile_signature_pdf_journey' if args.flow == 'signature' else 'resource_gated_rendered_document_pdf_journey'
        receipt['run'] = run([shell,'-NoProfile','-File',str(REPO/'apps/web_app/scripts/build-windows.ps1'),'-Test','-TestFilter',test,'-Ignored','-NoCapture'],env,log,600)
        text = log.read_text(encoding='utf-8'); matches = re.findall(r'Document journey evidence: (.+)',text)
        if len(matches)==1:
            native=Path(matches[0].strip()).resolve(strict=True)
            if not native.is_relative_to((REPO/'target').resolve(strict=True)):raise RuntimeError('Evidence escaped target')
            receipt['native']=str(native)
            result=native/'result.private.json'
            if result.is_file():receipt['native_sha256']=hashlib.sha256(result.read_bytes()).hexdigest()
        if receipt['run']['exit_code']:raise RuntimeError('Document journey failed; inspect '+str(log))
        if len(matches)!=1:raise RuntimeError('Missing native evidence')
        receipt['tests']=test_denominator(text,'rust',1)
        if hashes()!=receipt['sources']:raise RuntimeError('Selected source changed during the run')
        receipt['complete']=True
    finally:
        receipt['memory']=memory.finish()
        (folder/'result.private.json').write_text(json.dumps(receipt,indent=2),encoding='utf-8')
        print('Document journey qualification:',folder)


if __name__=='__main__':main()
