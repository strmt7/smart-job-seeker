#!/usr/bin/env python3
"""Validate handoff contracts and links, NOT a production application.
Optional authoring/QA tool: Python 3 + jsonschema. Product runtime remains Rust.
"""
from __future__ import annotations
import copy
import json
import re
from pathlib import Path
from jsonschema import Draft202012Validator, FormatChecker

ROOT = Path(__file__).resolve().parents[1]

def load(path: str):
    return json.loads((ROOT / path).read_text(encoding='utf-8'))

def main() -> None:
    results=[]
    mappings={
        'job_observation':'job_observation', 'claim':'claim',
        'prepared_application':'application_intent',
        'uncertain_application':'application_intent',
        'confirmed_application':'application_intent',
        'approval_grant':'approval_grant', 'receipt':'receipt',
        'reasoning_run':'reasoning_run',
    }
    for fixture,contract in mappings.items():
        validator=Draft202012Validator(load(f'schemas/{contract}.schema.json'),format_checker=FormatChecker())
        validator.check_schema(validator.schema)
        validator.validate(load(f'fixtures/{fixture}.json'))
        results.append({'test':f'valid fixture: {fixture}','passed':True})
    negatives=[]
    x=load('fixtures/confirmed_application.json');x['receipt_id']=None
    negatives.append(('confirmed requires receipt reference','application_intent',x))
    x=load('fixtures/claim.json');x['status']='externally_supported';x['evidence_ids']=[]
    negatives.append(('external support requires external evidence','claim',x))
    x=load('fixtures/reasoning_run.json');x['reasoning_enabled']=False
    negatives.append(('reasoning capability required','reasoning_run',x))
    x=load('fixtures/reasoning_run.json');x['research_evidence_ids']=[]
    negatives.append(('current external decision requires research evidence','reasoning_run',x))
    x=load('fixtures/approval_grant.json');x['explicit_user_gesture']=False
    negatives.append(('grant requires explicit gesture','approval_grant',x))
    x=load('fixtures/receipt.json');x['kind']='user_attestation';x['verification_status']='qualified'
    negatives.append(('attestation is not employer qualification','receipt',x))
    for name,contract,fixture in negatives:
        validator=Draft202012Validator(load(f'schemas/{contract}.schema.json'),format_checker=FormatChecker())
        assert list(validator.iter_errors(fixture)), f'Negative case wrongly accepted: {name}'
        results.append({'test':name,'passed':True})
    sources={s['id'] for s in load('research/sources.json')}
    for file in ROOT.rglob('*.md'):
        for sid in re.findall(r'\bS\d{2,3}\b',file.read_text(encoding='utf-8')):
            assert sid in sources, f'Unresolved source ID {sid} in {file}'
    results.append({'test':'all Markdown source IDs resolve','passed':True})
    tasks={t['id'] for t in load('agent/backlog.json')}
    for screen in load('design/screen_catalog.json'):
        for key in ['png','svg']:
            assert (ROOT/'design'/screen[key]).is_file(), f'Missing {screen[key]}'
        assert set(screen['task_ids']) <= tasks
    results.append({'test':'40 screens resolve assets and task IDs','passed':True})
    assert len(load('design/screen_catalog.json'))==40
    assert all(t['status']=='PLANNED' for t in load('agent/backlog.json'))
    results.append({'test':'implementation tasks are not falsely marked complete','passed':True})
    report={'scope':'Design-time structural checks only; no model, Rust runtime, Windows app or ATS execution tested.','results':results,'passed':len(results),'failed':0}
    (ROOT/'qa/contract_validation.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps(report,indent=2))

if __name__=='__main__':
    main()
