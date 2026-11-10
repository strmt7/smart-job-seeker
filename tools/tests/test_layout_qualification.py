"""Rendered checks require explicit evidence and confirmed owned cleanup."""
import copy
import importlib.util
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

TOOLS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('layout_qualification', TOOLS / 'qualify_layout.py')
qualification = importlib.util.module_from_spec(spec)
with patch.object(sys, 'path', [str(TOOLS), *sys.path]):
    spec.loader.exec_module(qualification)


class LayoutQualificationTests(unittest.TestCase):
    def visual(self):
        return {'complete': True, 'errors': [], 'cases': ['fictional rendered journey'],
                'cleanup': {'errors': [], 'appExited': True, 'contextClosed': True, 'browserDisconnected': True}}

    def test_incomplete_visual_runs_and_failed_or_unknown_cleanup_never_pass(self):
        qualification.check_native_result('visual-followups', self.visual())
        for field, value in [('complete',False),('complete','true'),('errors',['failure']),
                             ('error','native failure'),('cases',[]),('cases','not a denominator')]:
            report=self.visual();report[field]=value
            with self.subTest(field=field), self.assertRaises(RuntimeError):
                qualification.check_native_result('visual-followups',report)
        for field in ['appExited','contextClosed','browserDisconnected','errors']:
            for value in [None,False,'unknown']:
                report=self.visual();report['cleanup'][field]=value
                with self.subTest(field=field,value=value), self.assertRaises(RuntimeError):
                    qualification.check_native_result('visual-followups',report)
        with self.assertRaises(RuntimeError): qualification.check_native_result('visual-followups',[])

    def test_layout_checks_retain_their_existing_failure_and_cleanup_requirements(self):
        report={'completed':True,'failures':[],'scriptErrors':[],'cleanup':{'errors':[],'appExited':True}}
        qualification.check_native_result('layout',report)
        for field in ['failures','scriptErrors']:
            invalid=copy.deepcopy(report);invalid[field]=['failure']
            with self.assertRaises(RuntimeError): qualification.check_native_result('layout',invalid)
        report['cleanup']['appExited']=False
        with self.assertRaises(RuntimeError): qualification.check_native_result('layout',report)
        with self.assertRaises(ValueError): qualification.check_native_result('unknown',{})

    def test_unsupported_fast_visual_mode_cannot_start_a_child(self):
        with patch.object(sys,'argv',['qualify_layout.py','--flow','visual-followups','--quick']), \
                patch.object(qualification.shutil,'which',return_value='node'), \
                patch.object(qualification,'run') as run, self.assertRaises(SystemExit):
            qualification.main()
        run.assert_not_called()


if __name__ == '__main__': unittest.main()
