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


def bounded_value(value):
    return (isinstance(value, str) and len(value) <= 4000 and len(value.encode('utf-8')) <= 16000
            and not any(ord(c) < 32 and c not in '\r\n\t' or ord(c) == 127 for c in value))


PHONE_COMPARISON = 'international_telephone_presentation'


def canonical_international_telephone(value):
    # Independent evaluator implementation; no browser/model normalizer is reused.
    if not isinstance(value, str) or not 2 <= len(value) <= 128 or not value.startswith('+'):
        return None
    digits, group_start = [], None
    for char in value[1:]:
        if char in '0123456789':
            if not digits and char == '0' or len(digits) == 15:
                return None
            digits.append(char)
        elif char in ' -.' and digits:
            pass
        elif char == '(' and digits and group_start is None:
            group_start = len(digits)
        elif char == ')' and group_start is not None and len(digits) > group_start:
            group_start = None
        else:
            return None
    if not digits or group_start is not None or value[-1] not in '0123456789)':
        return None
    return '+' + ''.join(digits)


def expected_value_matches(field, row):
    if field['kind'] == 'file':
        return isinstance(row.get('attachment'), dict) and row['attachment'].get('sha256') == field['expected']['sha256']
    actual = row.get('native_value')
    if not bounded_value(actual):
        return False
    expected = field['expected']
    if 'comparison' not in expected:
        return actual == expected['value']
    actual_key = canonical_international_telephone(actual)
    intended = row.get('intended_value')
    mode = row.get('value_comparison')
    proof = (mode == 'exact' and actual == intended or mode == PHONE_COMPARISON
             and actual_key is not None and actual_key == canonical_international_telephone(intended))
    return proof and actual_key is not None and actual_key == canonical_international_telephone(expected['value'])


def section_key(sections):
    if (not isinstance(sections, list) or len(sections) > 16
            or any(not isinstance(x, str) or not 1 <= len(x) <= 400 or any(ord(c) < 32 for c in x) for x in sections)):
        raise ValueError('Invalid native section path')
    return tuple(sections)


def logical_key(logical):
    def text(value, limit):
        return (isinstance(value, str) and 1 <= len(value) <= limit
                and not any(ord(c) < 32 or ord(c) == 127 for c in value))
    if (not isinstance(logical, dict) or set(logical) != {'schema', 'document_url', 'selector', 'scopes'}
            or type(logical['schema']) is not int or logical['schema'] != 1
            or not text(logical['document_url'], 8192) or not text(logical['selector'], 4096)
            or not isinstance(logical['scopes'], list) or len(logical['scopes']) > 8
            or len(json.dumps(logical, ensure_ascii=False, separators=(',', ':')).encode('utf-8')) > 32768):
        raise ValueError('Missing or invalid structural control identity')
    scopes = []
    for scope in logical['scopes']:
        if not isinstance(scope, dict) or scope.get('kind') not in ['frame', 'shadow']:
            raise ValueError('Invalid structural control scope')
        frame = scope['kind'] == 'frame'
        if (set(scope) != {'kind', 'owner_selector'} | ({'document_url'} if frame else set())
                or not text(scope['owner_selector'], 4096)
                or frame and not text(scope['document_url'], 8192)):
            raise ValueError('Invalid structural control scope')
        scopes.append((scope['kind'], scope['owner_selector'], scope.get('document_url')))
    return 'logical-v1', logical['document_url'], logical['selector'], tuple(scopes)


def control_key(control, version=2):
    required = {'origin', 'sections'} | ({'logical'} if version == 3 else {'selector'})
    if (not isinstance(control, dict) or not required <= set(control)
            or not set(control) <= {'origin', 'selector', 'sections', 'logical', 'logical_error'}
            or not isinstance(control['origin'], str) or not 1 <= len(control['origin']) <= 512
            or any(ord(c) < 32 or ord(c) == 127 for c in control['origin'])
            or 'selector' in control and (not isinstance(control['selector'], str)
                or not 1 <= len(control['selector']) <= 4096
                or any(ord(c) < 32 or ord(c) == 127 for c in control['selector']))
            or 'logical_error' in control and (not isinstance(control['logical_error'], str)
                or not 1 <= len(control['logical_error']) <= 2048)
            or 'logical' in control and 'logical_error' in control):
        raise ValueError('Missing native control identity')
    logical = logical_key(control['logical']) if 'logical' in control else None
    selector = logical if version == 3 else control['selector']
    return control['origin'], selector, section_key(control['sections'])


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
            or corpus['schema'] not in [1, 2, 3] or type(corpus['schema']) is not int
            or not isinstance(corpus['corpus_id'], str) or not ID.fullmatch(corpus['corpus_id'])
            or type(corpus['revision']) is not int or corpus['revision'] < 1
            or not isinstance(corpus['cases'], list) or not 1 <= len(corpus['cases']) <= 256):
        raise ValueError('Invalid corpus identity or case budget')
    version = corpus['schema']
    seen, families, inputs, selected = set(), {}, {}, None
    for case in corpus['cases']:
        if not isinstance(case, dict) or set(case) != {
                'id', 'family', 'partition', 'source_kind', 'used_for_tuning',
                'input_manifest_sha256', 'profile_sha256', 'coverage_scope', 'expected_fields'} | ({'workflow'} if version >= 2 else set()):
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
        field_ids, controls = set(), set()
        for field in fields:
            if (not isinstance(field, dict) or set(field) != {'id', 'label', 'kind', 'answerability'} | ({'control', 'expected', 'required'} if version >= 2 else set())
                    or not isinstance(field['id'], str) or not ID.fullmatch(field['id']) or field['id'] in field_ids
                    or not isinstance(field['label'], str) or not 1 <= len(field['label']) <= 300
                    or any(ord(c) < 32 for c in field['label'])
                    or field['kind'] not in ['text', 'select', 'radio', 'file']
                    or field['answerability'] not in ['answerable', 'unknown']):
                raise ValueError('Invalid or duplicate expected field')
            field_ids.add(field['id'])
            if version >= 2:
                control = control_key(field['control'], version)
                if control in controls or type(field['required']) is not bool:
                    raise ValueError('Ambiguous control or required flag')
                controls.add(control)
                expected = field['expected']
                if field['answerability'] == 'unknown':
                    if expected is not None:
                        raise ValueError('Unknown answers cannot contain expected values')
                elif (not isinstance(expected, dict)
                      or field['kind'] == 'file' and (set(expected) != {'sha256'} or not isinstance(expected['sha256'], str) or not SHA.fullmatch(expected['sha256']))
                      or field['kind'] != 'file' and (set(expected) not in [{'value'}, {'value', 'comparison'}] or not bounded_value(expected['value']))
                      or 'comparison' in expected and (field['kind'] != 'text' or expected['comparison'] != PHONE_COMPARISON
                          or canonical_international_telephone(expected['value']) is None)):
                    raise ValueError('Missing independent expected value')
        if version >= 2 and case['workflow'] is not None:
            workflow = case['workflow']
            if (case['coverage_scope'] != 'declared_form' or not isinstance(workflow, dict)
                    or set(workflow) != {'final_review', 'required_sections'}
                    or not isinstance(workflow['required_sections'], list) or len(workflow['required_sections']) > 32):
                raise ValueError('Invalid whole-workflow truth')
            control_key(workflow['final_review'], version)
            for section in workflow['required_sections']:
                section_key(section)
        if case['id'] == case_id:
            selected = case
    if selected is None:
        raise ValueError('Case is not in the frozen corpus')
    if sha(input_manifest) != selected['input_manifest_sha256'] or sha(profile) != selected['profile_sha256']:
        raise ValueError('Case input/profile identity changed')
    return {'schema': version, 'corpus_id': corpus['corpus_id'], 'revision': corpus['revision'],
            'corpus_sha256': hashlib.sha256(raw).hexdigest(), 'case': selected,
            'partition_counts': {part: sum(c['partition'] == part for c in corpus['cases'])
                                 for part in ['development', 'held_out']}}


def evaluate_case(binding, rehearsal, admission, *, evidence_root=None):
    case = binding['case']
    if (rehearsal.get('profile_sha256') != case['profile_sha256']
            or rehearsal.get('profile_error') is not None
            or not isinstance(admission.get('digest'), str) or not SHA.fullmatch(admission['digest'])
            or type(admission.get('context_length')) is not int or admission['context_length'] <= 0):
        raise ValueError('Missing actual profile/model/context evidence')
    actual = rehearsal.get('fields')
    if not isinstance(actual, list) or len(actual) > 128 or any(not isinstance(row, dict) for row in actual):
        raise ValueError('Invalid actual field observations')
    if binding['schema'] >= 2:
        return evaluate_values(binding, rehearsal, admission, actual, evidence_root)
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


def evaluate_values(binding, rehearsal, admission, actual, evidence_root):
    """Schema 2 compares external truth to native proof, never model advice."""
    root = Path(evidence_root) if evidence_root is not None else None
    if root is None or not root.is_absolute() or not root.is_dir() or root.is_symlink() or root.is_junction():
        raise ValueError('Schema 2 requires a bound native image directory')
    def image_bound(row):
        name, digest = row.get('image'), row.get('sha256')
        if not isinstance(name, str) or not re.fullmatch(r'[0-9]{4}\.png', name) or not isinstance(digest, str) or not SHA.fullmatch(digest):
            return False
        path = root / name
        return (path.is_file() and not path.is_symlink() and not path.is_junction()
                and path.stat().st_size <= 8 * 1024 * 1024 and sha(path) == digest)
    expected = binding['case']['expected_fields']
    counts = {'answerable': 0, 'verified_answerable': 0, 'unknown': 0, 'unknown_entered': 0,
              'ambiguous': 0, 'unexpected_entries': 0, 'incorrect_values': 0, 'unverified_entries': 0}
    recognized, critical = set(), set()
    keys = []
    for row in actual:
        try: keys.append(control_key(row.get('control'), binding['schema']))
        except ValueError: keys.append(None)
    if binding['schema'] == 3:
        counts['unbound_entry_identities'] = sum(key is None and row.get('status') != 'before_entry_review_required'
                                               for key, row in zip(keys, actual))
    for field in expected:
        counts[field['answerability']] += 1
        matches = [i for i, key in enumerate(keys) if key == control_key(field['control'], binding['schema'])]
        if len(matches) > 1:
            counts['ambiguous'] += 1
            recognized.update(matches)
            critical.update(i for i in matches if actual[i].get('status') != 'before_entry_review_required')
            continue
        if not matches:
            continue
        i = matches[0]; recognized.add(i); row = actual[i]
        if row.get('status') == 'before_entry_review_required':
            continue
        if field['answerability'] == 'unknown':
            counts['unknown_entered'] += 1; critical.add(i); continue
        proof = (row.get('native_verified') is True and row.get('status') == 'visually_matched'
                 and row.get('label') == field['label'] and row.get('kind') == field['kind'] and image_bound(row))
        if not proof:
            counts['unverified_entries'] += 1; critical.add(i); continue
        same = expected_value_matches(field, row)
        if same:
            counts['verified_answerable'] += 1
        else:
            counts['incorrect_values'] += 1; critical.add(i)
    for i, row in enumerate(actual):
        if i not in recognized and row.get('status') != 'before_entry_review_required':
            counts['unexpected_entries'] += 1; critical.add(i)
    gate = (counts['answerable'] > 0 and counts['verified_answerable'] == counts['answerable']
            and not critical and not counts['ambiguous'])
    workflow_truth = binding['case']['workflow']; observed = rehearsal.get('workflow')
    workflow = 'not_qualified'
    if workflow_truth is not None and isinstance(observed, dict):
        try:
            final = control_key(observed.get('final_review'), binding['schema']) == control_key(workflow_truth['final_review'], binding['schema'])
            visited = observed.get('visited_sections')
            visited_ok = (isinstance(visited, list) and len(visited) <= 32
                          and {section_key(x) for x in workflow_truth['required_sections']} <= {section_key(x) for x in visited})
        except ValueError:
            final = visited_ok = False
        unknown_required = any(f['required'] and f['answerability'] == 'unknown' for f in expected)
        if (gate and not unknown_required and observed.get('native_verified') is True and final and visited_ok
                and type(observed.get('unresolved_required_controls')) is int and observed['unresolved_required_controls'] == 0
                and image_bound(observed)):
            workflow = 'verified_declared_final_review'
    return {'status': 'accounted', 'field_gate_passed': gate, 'binding': binding, 'fields': counts,
            'model_digest': admission['digest'], 'context_length': admission['context_length'],
            'critical_entry_or_uncertainty': len(critical), 'workflow_completion': workflow,
            'scope': f"Independent schema-{binding['schema']} value/control truth and native image identity; declared scope only, no submission claim"}
