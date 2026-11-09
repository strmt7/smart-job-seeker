"""Portable archive/continuous-broker tests. Fictional resources only."""
import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SOURCE=Path(__file__).resolve().parents[1]/'src/rehearsal_replay.py'
spec=importlib.util.spec_from_file_location('rehearsal_replay',SOURCE)
replay=importlib.util.module_from_spec(spec);spec.loader.exec_module(replay)
digest=lambda data:hashlib.sha256(data).hexdigest()


class Fixture:
    def __init__(self):
        self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name)
        self.resources=[];self.files={}
        self.add('https://fixture.invalid/form','index.html',b'<!doctype html><label>Fictional form</label>',content_type='text/html')
    def add(self,url,name,data,method='GET',post=b'',content_type='application/json',headers=None,status=200):
        (self.root/name).parent.mkdir(parents=True,exist_ok=True);(self.root/name).write_bytes(data);self.files[name]=digest(data)
        self.resources.append({'url':url,'method':method,'request_body_sha256':digest(post),'status':status,
                               'headers':headers if headers is not None else [{'name':'Content-Type','value':content_type}],
                               'body_file':name,'body_sha256':digest(data)})
    def archive(self,changes=None):
        value={'schema':1,'body_representation':'decoded_http','entry_url':'https://fixture.invalid/form','resources':self.resources}
        if changes:value.update(changes)
        data=json.dumps(value).encode();(self.root/'replay.json').write_bytes(data);self.files['replay.json']=digest(data)
        return replay.Archive(self.root,'replay.json',self.files,'index.html')
    def close(self):self.temp.cleanup()


class ReplayTests(unittest.TestCase):
    def setUp(self):self.fixture=Fixture()
    def tearDown(self):self.fixture.close()
    def test_decoded_response_preserves_bytes_and_repairs_transport_headers(self):
        self.fixture.add('https://fixture.invalid/style','style.bin',b'body{color:green}',headers=[{'name':'Content-Encoding','value':'gzip'},{'name':'Content-Length','value':'99'},{'name':'Content-Type','value':'text/css'}])
        archive=self.fixture.archive();response,reason=archive.response({'url':'https://fixture.invalid/style','method':'GET'})
        self.assertEqual(reason,'recorded_response');self.assertEqual(base64.b64decode(response['body']),b'body{color:green}')
        self.assertEqual({h['name'].lower():h['value'] for h in response['responseHeaders']},{'content-type':'text/css','content-length':'17'})
    def test_method_query_and_exact_post_bytes_form_cache_identity(self):
        self.fixture.add('https://fixture.invalid/query?a=1&b=2','data.bin',b'{"ok":true}',method='POST',post=b'{"query":"bootstrap"}')
        archive=self.fixture.archive();request={'url':'https://fixture.invalid/query?a=1&b=2','method':'POST','postData':'{"query":"bootstrap"}','hasPostData':True}
        self.assertIsNotNone(archive.response(request)[0])
        for changes in [{'method':'GET'},{'url':'https://fixture.invalid/query?b=2&a=1'},{'postData':'{"query":"candidate answer"}'}]:
            with self.subTest(changes=changes):self.assertEqual(archive.response({**request,**changes})[1],'response_not_recorded')
    def test_binary_multipart_missing_and_oversize_request_bodies_are_not_guessed(self):
        archive=self.fixture.archive()
        for fields in [{'hasPostData':True},{'postDataEntries':[{}]}, {'headers':{'Content-Type':'multipart/form-data; boundary=1'}},{'headers':[]},{'postData':3}]:
            self.assertEqual(archive.response({'url':'https://fixture.invalid/form','method':'GET',**fields})[1],'opaque_request_body')
    def test_complete_byte_entries_match_only_exact_recorded_body(self):
        body=b'{"query":"bootstrap"}'
        self.fixture.add('https://fixture.invalid/query','post.bin',b'{}',method='POST',post=body)
        archive=self.fixture.archive()
        request={'url':'https://fixture.invalid/query','method':'POST','hasPostData':True,
                 'postDataEntries':[{'bytes':base64.b64encode(body[:5]).decode()},{'bytes':base64.b64encode(body[5:]).decode()}]}
        self.assertIsNotNone(archive.response(request)[0])
        self.assertIsNotNone(archive.response({**request,'postData':body.decode()})[0])
        for extra in [{'postData':'different'}, {'postDataEntries':[{}]}, {'postDataEntries':[]},
                      {'postDataEntries':[{'bytes':'%%%'}]}, {'postDataEntries':[{'bytes':'QQ==','file':'ignored?'}]}]:
            self.assertEqual(archive.response({**request,**extra})[1],'opaque_request_body')

    def test_request_and_manifest_resource_budgets_are_enforced(self):
        archive=self.fixture.archive()
        with patch.object(replay,'MAX_BODY',64):
            for fields in [{'postData':'a'*65}, {'postData':'資格'*32},
                           {'postDataEntries':[{'bytes':base64.b64encode(b'a'*65).decode()}]},
                           {'postDataEntries':[{'bytes':'YQ=='}]*257}]:
                self.assertEqual(archive.response({'url':'https://fixture.invalid/form','method':'POST',**fields})[1],'opaque_request_body')
        with patch.object(replay,'MAX_TOTAL',1),self.assertRaisesRegex(replay.ReplayError,'budget'):
            self.fixture.archive()
        self.fixture.resources*=513
        with self.assertRaisesRegex(replay.ReplayError,'manifest'):
            self.fixture.archive()

    def test_duplicate_manifest_keys_cannot_silently_replace_a_resource(self):
        self.fixture.archive()
        path=self.fixture.root/'replay.json';data=path.read_bytes().replace(b'"schema": 1',b'"schema": 1, "schema": 1')
        path.write_bytes(data);self.fixture.files['replay.json']=digest(data)
        with self.assertRaisesRegex(replay.ReplayError,'duplicate_replay_json_key'):
            replay.Archive(self.fixture.root,'replay.json',self.fixture.files,'index.html')

    def test_unknown_url_and_external_protocols_never_gain_a_live_fallback(self):
        archive=self.fixture.archive()
        for url in ['file:///C:/private','http://127.0.0.1/submit','mailto:fictional@example.invalid','https://user:pass@fixture.invalid/','https://fixture.invalid:444/']:
            self.assertEqual(archive.response({'url':url,'method':'GET'})[1],'unsupported_url')
        self.assertEqual(archive.response({'url':'https://fixture.invalid/submit','method':'POST'})[1],'response_not_recorded')
    def test_changed_file_and_inventory_mismatch_are_refused(self):
        archive=self.fixture.archive();(self.fixture.root/'index.html').write_bytes(b'changed')
        with self.assertRaisesRegex(replay.ReplayError,'replay_body_changed'):archive.response({'url':'https://fixture.invalid/form','method':'GET'})
        with self.assertRaises(replay.ReplayError):self.fixture.archive()
    def test_duplicate_credentials_partial_and_unknown_manifest_fields_are_refused(self):
        self.fixture.resources.append(dict(self.fixture.resources[0]))
        with self.assertRaisesRegex(replay.ReplayError,'ambiguous'):self.fixture.archive()
        self.fixture.resources.pop()
        for name,value in [('Set-Cookie','session=fictional'),('Authorization','fictional'),('X-Test','line\nvalue')]:
            original=self.fixture.resources[0]['headers'];self.fixture.resources[0]['headers']=[{'name':name,'value':value}]
            with self.assertRaises(replay.ReplayError):self.fixture.archive()
            self.fixture.resources[0]['headers']=original
        for status in [True,101,206,304,600]:
            self.fixture.resources[0]['status']=status
            with self.assertRaises(replay.ReplayError):self.fixture.archive()
        self.fixture.resources[0]['status']=200
        with self.assertRaises(replay.ReplayError):self.fixture.archive({'unknown':'ignored?'})
    def test_paths_and_entry_scope_cannot_escape(self):
        for value in ['../file','/file','C:/file','a\\b','file:stream','CON.txt','COM¹.txt','name.','a//b']:
            with self.subTest(path=value),self.assertRaises(replay.ReplayError):replay.relative(value)
        for entry in ['http://fixture.invalid/form','https://fixture.invalid/missing']:
            with self.assertRaises(replay.ReplayError):self.fixture.archive({'entry_url':entry})
    def test_head_length_and_fragment_routing_are_preserved(self):
        self.fixture.add('https://fixture.invalid/file','empty.bin',b'',method='HEAD',headers=[{'name':'Content-Length','value':'42'}])
        archive=self.fixture.archive({'entry_url':'https://fixture.invalid/form#application'})
        value,_=archive.response({'url':'https://fixture.invalid/file','method':'HEAD'})
        self.assertEqual(value['responseHeaders'],[{'name':'Content-Length','value':'42'}])


class BrokerTests(unittest.TestCase):
    def setUp(self):
        self.fixture=Fixture();self.sent=[];self.emitted=[];self.broker=replay.Broker(self.fixture.archive(),self.sent.append,self.emitted.append)
    def tearDown(self):self.fixture.close()
    def ack(self,error=None):
        command=self.sent[-1];reply={'id':command['id'],'result':{}}
        if error:reply['error']={'message':error}
        self.assertFalse(self.broker.browser(reply));return command
    def bind(self):
        self.assertTrue(self.broker.host({'id':1,'method':'Target.attachToTarget','params':{'targetId':'root'}}))
        original={'id':1,'result':{'sessionId':'page'}}
        self.assertFalse(self.broker.browser(original));self.assertEqual(self.sent[-1]['method'],'Fetch.enable');self.assertEqual(self.emitted,[])
        self.ack();self.assertEqual(self.sent[-1]['method'],'Target.setAutoAttach');self.assertEqual(self.emitted,[])
        self.ack();self.assertEqual(self.emitted,[original])
    def test_attachment_is_held_until_interception_and_auto_attach_are_acknowledged(self):self.bind()
    def test_fetch_requests_are_serviced_without_a_host_command(self):
        self.bind();self.emitted.clear();event={'method':'Fetch.requestPaused','sessionId':'page','params':{'requestId':'r','request':{'url':'https://fixture.invalid/form','method':'GET'}}}
        self.assertFalse(self.broker.browser(event));self.assertEqual(self.sent[-1]['method'],'Fetch.fulfillRequest');self.ack();self.assertEqual(self.broker.report()['served'],1)
        event['params']['request']['url']='https://fixture.invalid/submit?email=fictional@example.invalid';event['params']['request']['method']='POST'
        self.assertFalse(self.broker.browser(event));self.assertEqual(self.sent[-1]['method'],'Fetch.failRequest');self.ack();report=self.broker.report();self.assertEqual(report['blocked'],1);self.assertNotIn('fictional@example.invalid',json.dumps(report))
    def test_report_counts_only_acknowledged_responses_and_failed_commands_abort(self):
        self.bind()
        self.broker.browser({'method':'Fetch.requestPaused','sessionId':'page','params':{'requestId':'r','request':{'url':'https://fixture.invalid/form','method':'GET'}}})
        self.assertEqual(self.broker.report()['served'],0)
        with self.assertRaisesRegex(replay.ReplayError,'interception_failed'):
            self.ack('Request was canceled')
        self.assertEqual(self.broker.report()['served'],0)

    def test_child_page_waits_for_interception_before_resuming(self):
        self.bind();event={'method':'Target.attachedToTarget','params':{'sessionId':'child','waitingForDebugger':True,'targetInfo':{'type':'iframe','targetId':'frame-target'}}}
        self.assertFalse(self.broker.browser(event));self.ack();self.ack();self.assertEqual(self.sent[-1]['method'],'Runtime.runIfWaitingForDebugger');self.assertNotIn(event,self.emitted);self.ack();self.assertIn(event,self.emitted)
    def test_manual_frame_alias_reuses_primary_interceptor(self):
        self.bind()
        event={'method':'Target.attachedToTarget','params':{'sessionId':'child','waitingForDebugger':True,'targetInfo':{'type':'iframe','targetId':'frame-target'}}}
        self.broker.browser(event)
        self.broker.host({'id':2,'method':'Target.attachToTarget','params':{'targetId':'frame-target'}})
        alias={'id':2,'result':{'sessionId':'alias'}}
        self.broker.browser(alias)
        self.assertNotIn(alias,self.emitted)
        self.ack();self.ack();self.ack()
        self.assertIn(event,self.emitted);self.assertIn(alias,self.emitted)
        self.assertEqual(len([c for c in self.sent if c['method']=='Fetch.enable']),2)
        self.broker.host({'id':3,'method':'Target.attachToTarget','params':{'targetId':'frame-target'}})
        later={'id':3,'result':{'sessionId':'later'}}
        self.broker.browser(later);self.assertIn(later,self.emitted)
        self.assertEqual(len([c for c in self.sent if c['method']=='Fetch.enable']),2)

    def test_worker_limits_are_explicit_instead_of_claiming_full_replay(self):
        self.bind();event={'method':'Target.attachedToTarget','params':{'sessionId':'worker','waitingForDebugger':True,'targetInfo':{'type':'worker','targetId':'worker-target'}}}
        self.broker.browser(event);self.assertEqual(self.sent[-1]['method'],'Target.setAutoAttach');self.ack('unsupported target');self.ack()
        report=self.broker.report();self.assertEqual(report['unsupported_targets'],{'worker':1});self.assertEqual(report['unmanaged_child_sessions'],1)
    def test_interception_error_override_duplicate_ids_and_unknown_sessions_fail(self):
        for method in ['Fetch.disable','Fetch.continueRequest','Fetch.enable','Target.setAutoAttach']:
            with self.assertRaises(replay.ReplayError):self.broker.host({'id':1,'method':method})
        with self.assertRaises(replay.ReplayError):self.broker.host({'id':replay.INTERNAL_ID,'method':'Page.enable'})
        self.broker.host({'id':1,'method':'Target.attachToTarget','params':{'targetId':'root'}});self.broker.browser({'id':1,'result':{'sessionId':'page'}})
        with self.assertRaisesRegex(replay.ReplayError,'interception_failed'):self.ack('Fetch refused')
        with self.assertRaises(replay.ReplayError):self.broker.browser({'id':replay.INTERNAL_ID+99,'result':{}})
        with self.assertRaises(replay.ReplayError):self.broker.browser({'method':'Fetch.requestPaused','sessionId':'unknown','params':{'requestId':'x','request':{}}})


if __name__=='__main__':unittest.main()
