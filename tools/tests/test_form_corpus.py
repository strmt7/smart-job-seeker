"""Prevent corpus leakage, stale bindings and inflated form-coverage accounting."""
from pathlib import Path
import importlib.util
import tempfile
import unittest
import json
import copy

TOOLS=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('form_corpus',TOOLS/'form_corpus.py')
corpus=importlib.util.module_from_spec(spec);spec.loader.exec_module(corpus)

class CorpusTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name);self.input=self.root/'input.json';self.profile=self.root/'profile.json';self.path=self.root/'corpus.json'
        self.input.write_text('{"fixture":1}');self.profile.write_text('{"fictional":true}')
        self.case={'id':'case-one','family':'simple','partition':'development','source_kind':'fictional','used_for_tuning':True,
                   'coverage_scope':'declared_form','input_manifest_sha256':corpus.sha(self.input),'profile_sha256':corpus.sha(self.profile),
                   'expected_fields':[{'id':'email','label':'Email','kind':'text','answerability':'answerable'},
                                      {'id':'question','label':'Unknown question','kind':'text','answerability':'unknown'}]}
        self.data={'schema':1,'corpus_id':'qualification','revision':1,'cases':[self.case]}
        self.admission={'digest':'a'*64,'context_length':4096}
    def bind(self):
        self.path.write_text(json.dumps(self.data));return corpus.bind_case(self.path,'case-one',self.input,self.profile)
    def result(self,fields):
        return corpus.evaluate_case(self.bind(),{'profile_sha256':corpus.sha(self.profile),'fields':fields},self.admission)
    def test_missing_fields_remain_in_denominator_without_workflow_credit(self):
        result=self.result([]);self.assertEqual(result['fields']['answerable'],1);self.assertEqual(result['fields']['verified_answerable'],0)
        self.assertEqual(result['workflow_completion'],'not_qualified');self.assertFalse(result['field_gate_passed'])
    def test_only_unique_native_visual_matches_count_and_unknown_entries_are_critical(self):
        result=self.result([{'label':'Email','kind':'text','status':'visually_matched'},
                            {'label':'Unknown question','kind':'text','status':'visual_difference'}])
        self.assertEqual(result['fields']['verified_answerable'],1);self.assertEqual(result['critical_entry_or_uncertainty'],1)
    def test_ambiguous_and_wrong_kind_entries_do_not_become_correct(self):
        for fields in [[{'label':'Email','kind':'text','status':'visually_matched'}]*2,
                       [{'label':'Email','kind':'file','status':'visually_matched'}]]:
            result=self.result(fields);self.assertEqual(result['fields']['verified_answerable'],0)
            self.assertGreater(result['critical_entry_or_uncertainty'],0)
    def test_unknown_visual_refusal_is_not_an_entry(self):
        result=self.result([{'label':'Unknown question','status':'before_entry_review_required'}]);self.assertEqual(result['critical_entry_or_uncertainty'],0)
    def test_field_gate_requires_all_answerable_fields_without_unsafe_or_ambiguous_entries(self):
        result=self.result([{'label':'Email','kind':'text','status':'visually_matched'},
                            {'label':'Unknown question','status':'before_entry_review_required'}])
        self.assertTrue(result['field_gate_passed']);self.assertEqual(result['workflow_completion'],'not_qualified')
        result=self.result([{'label':'Email','kind':'text','status':'visually_matched'},
                            {'label':'Unknown question','kind':'text','status':'visually_matched'}])
        self.assertFalse(result['field_gate_passed'])

    def test_actual_profile_model_and_context_are_required(self):
        binding=self.bind()
        for admission,profile in [({'digest':None,'context_length':4096},corpus.sha(self.profile)),
                                  ({'digest':'a'*64,'context_length':0},corpus.sha(self.profile)),(self.admission,'b'*64)]:
            with self.assertRaises(ValueError):corpus.evaluate_case(binding,{'profile_sha256':profile,'fields':[]},admission)
    def test_input_and_profile_mutation_invalidate_case(self):
        self.bind();self.input.write_text('changed')
        with self.assertRaisesRegex(ValueError,'identity changed'):corpus.bind_case(self.path,'case-one',self.input,self.profile)
        self.input.write_text('{"fixture":1}');self.profile.write_text('changed')
        with self.assertRaisesRegex(ValueError,'identity changed'):corpus.bind_case(self.path,'case-one',self.input,self.profile)
    def test_family_and_input_leakage_are_rejected_across_partitions(self):
        for same_input in [True,False]:
            extra=copy.deepcopy(self.case);extra.update(id='second',partition='held_out',used_for_tuning=False)
            if same_input:extra['family']='different-family'
            else:extra['input_manifest_sha256']='b'*64
            self.data['cases']=[self.case,extra]
            with self.assertRaisesRegex(ValueError,'leakage'):self.bind()
    def test_tuning_and_duplicate_ids_cannot_hide_as_holdout(self):
        self.case['partition']='held_out'
        with self.assertRaisesRegex(ValueError,'Tuned'):self.bind()
        self.case['partition']='development';self.data['cases'].append(copy.deepcopy(self.case))
        with self.assertRaisesRegex(ValueError,'Duplicate case'):self.bind()
    def test_duplicate_json_keys_and_boolean_revision_are_rejected(self):
        self.path.write_text('{"schema":1,"schema":1}')
        with self.assertRaisesRegex(ValueError,'Duplicate corpus JSON'):corpus.bind_case(self.path,'case-one',self.input,self.profile)
        self.data['revision']=True
        with self.assertRaisesRegex(ValueError,'identity'):self.bind()

class ValueCorpusTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name);self.input=self.root/'input.json';self.profile=self.root/'profile.json';self.path=self.root/'corpus.json'
        self.input.write_text('{"fixture":2}');self.profile.write_text('{"fictional":true}')
        self.image=self.root/'0001.png';self.image.write_bytes(b'\x89PNG\r\n\x1a\nfictional-image-identity')
        self.control={'origin':'https://fixture.invalid','selector':'#email','sections':['Personal']}
        self.field={'id':'email','label':'Email','kind':'text','answerability':'answerable','required':True,
                    'control':self.control,'expected':{'value':'avery@example.invalid'}}
        self.case={'id':'case-two','family':'value-fixture','partition':'development','source_kind':'fictional','used_for_tuning':True,
                   'coverage_scope':'declared_form','input_manifest_sha256':corpus.sha(self.input),'profile_sha256':corpus.sha(self.profile),
                   'expected_fields':[self.field],'workflow':None}
        self.data={'schema':2,'corpus_id':'value-qualification','revision':1,'cases':[self.case]}
        self.admission={'digest':'a'*64,'context_length':4096}
        self.row={'label':'Email','kind':'text','control':copy.deepcopy(self.control),'status':'visually_matched',
                  'native_verified':True,'native_value':'avery@example.invalid','image':'0001.png','sha256':corpus.sha(self.image)}
    def bind(self):
        self.path.write_text(json.dumps(self.data));return corpus.bind_case(self.path,'case-two',self.input,self.profile)
    def evaluate(self,rows=None,workflow=None):
        return corpus.evaluate_case(self.bind(),{'profile_sha256':corpus.sha(self.profile),'fields':[self.row] if rows is None else rows,
                                                'workflow':workflow},self.admission,evidence_root=self.root)
    def test_independent_value_is_required_even_for_a_native_visual_match(self):
        self.assertTrue(self.evaluate()['field_gate_passed'])
        self.row['native_value']='wrong@example.invalid'
        result=self.evaluate();self.assertFalse(result['field_gate_passed'])
        self.assertEqual(result['fields']['incorrect_values'],1);self.assertEqual(result['critical_entry_or_uncertainty'],1)
    def test_missing_or_false_native_proof_is_unverified_not_correct(self):
        for proof in [None,False,1,'true']:
            self.row['native_verified']=proof
            self.assertEqual(self.evaluate()['fields']['unverified_entries'],1)
        self.row['native_verified']=True;self.row['status']='entry_unverified'
        self.assertEqual(self.evaluate()['critical_entry_or_uncertainty'],1)
    def test_control_identity_disambiguates_repeated_labels_without_order_credit(self):
        second=copy.deepcopy(self.field);second.update(id='second',control={**self.control,'selector':'#second'},expected={'value':'second@example.invalid'})
        self.case['expected_fields'].append(second)
        row2={**self.row,'control':copy.deepcopy(second['control']),'native_value':'second@example.invalid'}
        self.assertTrue(self.evaluate([row2,self.row])['field_gate_passed'])
        row2['native_value'],self.row['native_value']=self.row['native_value'],row2['native_value']
        self.assertEqual(self.evaluate([row2,self.row])['fields']['incorrect_values'],2)
    def test_changed_origin_section_or_selector_is_not_the_same_control(self):
        for change in [{'origin':'https://other.invalid'},{'selector':'#other'},{'sections':['Other']}]:
            row={**self.row,'control':{**self.control,**change}}
            result=self.evaluate([row]);self.assertFalse(result['field_gate_passed']);self.assertEqual(result['fields']['unexpected_entries'],1)
    def test_duplicate_truth_and_observations_cannot_inflate_coverage(self):
        second=copy.deepcopy(self.field);second['id']='second';self.case['expected_fields'].append(second)
        with self.assertRaisesRegex(ValueError,'Ambiguous control'):self.bind()
        self.case['expected_fields'].pop()
        result=self.evaluate([self.row,self.row]);self.assertEqual(result['fields']['ambiguous'],1);self.assertFalse(result['field_gate_passed'])
    def test_image_identity_and_directory_are_required(self):
        binding=self.bind()
        with self.assertRaisesRegex(ValueError,'bound native image'):corpus.evaluate_case(binding,{'profile_sha256':corpus.sha(self.profile),'fields':[self.row]},self.admission)
        self.image.write_bytes(b'changed')
        self.assertEqual(self.evaluate()['fields']['unverified_entries'],1)
        self.row['image']='../0001.png';self.assertFalse(self.evaluate()['field_gate_passed'])
    def test_file_truth_compares_verified_attachment_hash_not_filename(self):
        self.field.update(kind='file',expected={'sha256':'b'*64})
        self.row.update(kind='file',native_value='same-name.pdf',attachment={'sha256':'b'*64})
        self.assertTrue(self.evaluate()['field_gate_passed'])
        self.row['attachment']['sha256']='c'*64
        self.assertEqual(self.evaluate()['fields']['incorrect_values'],1)
    def test_unknown_answers_are_not_graded_as_available_or_autofilled(self):
        self.field.update(answerability='unknown',expected=None)
        self.assertEqual(self.evaluate()['fields']['unknown_entered'],1)
        self.row['status']='before_entry_review_required'
        self.assertEqual(self.evaluate()['critical_entry_or_uncertainty'],0)
        self.field['expected']={'value':'invented'}
        with self.assertRaisesRegex(ValueError,'Unknown'):self.bind()
    def test_full_workflow_requires_declared_final_review_and_remaining_control_truth(self):
        final={**self.control,'selector':'#review','sections':['Review']}
        self.case['workflow']={'final_review':final,'required_sections':[['Personal'],['Review']]}
        observed={'native_verified':True,'final_review':final,'visited_sections':[['Personal'],['Review']],
                  'unresolved_required_controls':0,'image':'0001.png','sha256':corpus.sha(self.image)}
        self.assertEqual(self.evaluate(workflow=observed)['workflow_completion'],'verified_declared_final_review')
        for change in [{'native_verified':False},{'visited_sections':[['Personal']]},{'unresolved_required_controls':1},
                       {'unresolved_required_controls':False},{'final_review':self.control},{'sha256':'0'*64}]:
            with self.subTest(change=change):self.assertEqual(self.evaluate(workflow={**observed,**change})['workflow_completion'],'not_qualified')
        self.case['coverage_scope']='initial_native_snapshot'
        with self.assertRaisesRegex(ValueError,'whole-workflow'):self.bind()
    def test_unknown_required_answer_cannot_be_hidden_by_a_workflow_success_flag(self):
        final={**self.control,'selector':'#review'}
        self.case['workflow']={'final_review':final,'required_sections':[]}
        unknown=copy.deepcopy(self.field);unknown.update(id='unknown',answerability='unknown',expected=None,control={**self.control,'selector':'#unknown'})
        self.case['expected_fields'].append(unknown)
        observed={'native_verified':True,'final_review':final,'visited_sections':[],'unresolved_required_controls':0,
                  'image':'0001.png','sha256':corpus.sha(self.image)}
        result=self.evaluate(workflow=observed)
        self.assertTrue(result['field_gate_passed']);self.assertEqual(result['workflow_completion'],'not_qualified')

if __name__=='__main__':unittest.main()
