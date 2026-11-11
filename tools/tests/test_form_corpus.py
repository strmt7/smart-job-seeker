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

if __name__=='__main__':unittest.main()
