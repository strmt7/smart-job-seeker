"""Source-bound metadata and conservative accounting for existing form qualifiers.

This module never runs browsers/models, supplies answers or fetches source pages.
Real source manifests and corpus files stay in ignored/private qualification data.
"""
from pathlib import Path
import hashlib
import json
import re

MAX_BYTES = 1024 * 1024
ID = re.compile(r"[a-z0-9][a-z0-9_.-]{0,95}\Z")
SHA = re.compile(r"[0-9a-f]{64}\Z")


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def bind_case(path, case_id, input_manifest, profile):
    with Path(path).open('rb') as stream:
        raw = stream.read(MAX_BYTES + 1)
    if len(raw) > MAX_BYTES:
        raise ValueError('Corpus exceeds the 1 MiB metadata budget')
    def unique_pairs(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError('Duplicate corpus JSON key')
            result[key] = value
        return result
    corpus = json.loads(raw, object_pairs_hook=unique_pairs)
    if (not isinstance(corpus, dict) or set(corpus) != {'schema', 'corpus_id', 'revision', 'cases'}
            or corpus['schema'] != 1 or type(corpus['schema']) is not int
            or not isinstance(corpus['corpus_id'], str) or not ID.fullmatch(corpus['corpus_id'])
            or type(corpus['revision']) is not int or corpus['revision'] < 1
            or not isinstance(corpus['cases'], list) or not 1 <= len(corpus['cases']) <= 256):
        raise ValueError('Invalid corpus identity or case budget')
    seen, families, inputs, selected = set(), {}, {}, None
    for case in corpus['cases']:
        if not isinstance(case, dict) or set(case) != {
                'id', 'family', 'partition', 'source_kind', 'used_for_tuning',
                'input_manifest_sha256', 'profile_sha256', 'coverage_scope', 'expected_fields'}:
            raise ValueError('Invalid case metadata')
        if any(not isinstance(case[key], str) or not ID.fullmatch(case[key]) for key in ['id', 'family']):
            raise ValueError('Invalid case/family identity')
        if case['id'] in seen:
            raise ValueError('Duplicate case identity')
        seen.add(case['id'])
        partition = case['partition']
        if partition not in ['development', 'held_out'] or case['source_kind'] not in ['fictional', 'saved_public']:
            raise ValueError('Invalid partition or source kind')
        if type(case['used_for_tuning']) is not bool or partition == 'held_out' and case['used_for_tuning']:
            raise ValueError('Tuned cases cannot be held out')
        for key in ['input_manifest_sha256', 'profile_sha256']:
            if not isinstance(case[key], str) or not SHA.fullmatch(case[key]):
                raise ValueError('Missing exact input identity')
        for registry, key in [(families, case['family']), (inputs, case['input_manifest_sha256'])]:
            if key in registry and registry[key] != partition:
                raise ValueError('Development/held-out input or family leakage')
            registry[key] = partition
        if case['coverage_scope'] not in ['initial_native_snapshot', 'declared_form']:
            raise ValueError('Missing explicit coverage scope')
        fields = case['expected_fields']
        if not isinstance(fields, list) or not 1 <= len(fields) <= 128:
            raise ValueError('Invalid expected field denominator')
        field_ids = set()
        for field in fields:
            if (not isinstance(field, dict) or set(field) != {'id', 'label', 'kind', 'answerability'}
                    or not isinstance(field['id'], str) or not ID.fullmatch(field['id']) or field['id'] in field_ids
                    or not isinstance(field['label'], str) or not 1 <= len(field['label']) <= 300
                    or any(ord(c) < 32 for c in field['label'])
                    or field['kind'] not in ['text', 'select', 'radio', 'file']
                    or field['answerability'] not in ['answerable', 'unknown']):
                raise ValueError('Invalid or duplicate expected field')
            field_ids.add(field['id'])
        if case['id'] == case_id:
            selected = case
    if selected is None:
        raise ValueError('Case is not in the frozen corpus')
    if sha(input_manifest) != selected['input_manifest_sha256'] or sha(profile) != selected['profile_sha256']:
        raise ValueError('Case input/profile identity changed')
    return {'schema': 1, 'corpus_id': corpus['corpus_id'], 'revision': corpus['revision'],
            'corpus_sha256': hashlib.sha256(raw).hexdigest(), 'case': selected,
            'partition_counts': {part: sum(c['partition'] == part for c in corpus['cases'])
                                 for part in ['development', 'held_out']}}


def evaluate_case(binding, rehearsal, admission):
    case = binding['case']
    if (rehearsal.get('profile_sha256') != case['profile_sha256']
            or rehearsal.get('profile_error') is not None
            or not isinstance(admission.get('digest'), str) or not SHA.fullmatch(admission['digest'])
            or type(admission.get('context_length')) is not int or admission['context_length'] <= 0):
        raise ValueError('Missing actual profile/model/context evidence')
    actual = rehearsal.get('fields')
    if not isinstance(actual, list) or len(actual) > 128 or any(not isinstance(row, dict) for row in actual):
        raise ValueError('Invalid actual field observations')
    expected = case['expected_fields']
    counts = {'answerable': 0, 'verified_answerable': 0, 'unknown': 0,
              'unknown_entered': 0, 'ambiguous': 0, 'unexpected_entries': 0}
    recognized = set()
    for field in expected:
        key = (field['label'], field['kind'])
        counts[field['answerability']] += 1
        # Repeated labels are unresolved rather than credited by order or count.
        if sum((f['label'], f['kind']) == key for f in expected) != 1:
            counts['ambiguous'] += 1
            continue
        rows = [(i, r) for i, r in enumerate(actual) if r.get('label') == field['label']]
        if len(rows) > 1:
            counts['ambiguous'] += 1
            continue
        if not rows:
            continue
        i, row = rows[0]
        uncertain_entry = row.get('status') != 'before_entry_review_required'
        if uncertain_entry and row.get('kind') != field['kind']:
            continue
        recognized.add(i)
        entered = row.get('status') == 'visually_matched'
        if field['answerability'] == 'unknown':
            counts['unknown_entered'] += int(uncertain_entry)
        else:
            counts['verified_answerable'] += int(entered and row.get('kind') == field['kind'])
    counts['unexpected_entries'] = sum(r.get('status') != 'before_entry_review_required' and i not in recognized
                                       for i, r in enumerate(actual))
    gate = (counts['answerable'] > 0 and counts['verified_answerable'] == counts['answerable']
            and counts['unknown_entered'] == 0 and counts['unexpected_entries'] == 0 and counts['ambiguous'] == 0)
    return {'status': 'accounted', 'field_gate_passed': gate, 'binding': binding, 'fields': counts,
            'model_digest': admission['digest'], 'context_length': admission['context_length'],
            'critical_entry_or_uncertainty': counts['unknown_entered'] + counts['unexpected_entries'],
            'workflow_completion': 'not_qualified',
            'scope': 'Declared fixed-fictional-input field records; no universal accuracy or workflow-success claim'}
