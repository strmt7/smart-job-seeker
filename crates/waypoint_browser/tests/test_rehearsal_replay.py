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
    def test_version3_binds_anonymous_asset_roles_mime_and_native_get_context(self):
        roles=[('script','Script','text/javascript'),('stylesheet','Stylesheet','text/css'),('font','Font','font/woff2'),
               ('image','Image','image/svg+xml'),('json','Fetch','application/json'),('manifest','Manifest','application/manifest+json')]
        for index,(kind,resource_type,mime) in enumerate(roles):
            url='https://cdn.fixture.invalid/asset-'+str(index);body=('fictional-'+kind).encode()
            self.fixture.add(url,str(index)+'.bin',body,headers=[{'name':'Content-Type','value':mime},{'name':'Vary','value':'Origin'}])
            self.fixture.resources[-1].update(request_origin_absent=True,request_origin_absent_asset=kind)
            archive=self.fixture.archive({'schema':3});request={'url':url,'method':'GET','headers':{'Accept':'*/*'}}
            reply,reason=archive.response(request,resource_type);self.assertEqual(reason,'recorded_response');self.assertEqual(base64.b64decode(reply['body']),body)
            if kind=='json':self.assertIsNotNone(archive.response(request,'XHR')[0])
            for wrong in ['Document','Other','Media',None]+[role[1] for role in roles if role[0]!=kind]:
                self.assertIsNone(archive.response(request,wrong)[0],(kind,wrong))
            for headers in [{},{'Origin':'https://fixture.invalid'},{'Cookie':''},{'Authorization':''},{'Proxy-Authorization':''},
                            {'Accept':'*/*','accept':'*/*'},{'Accept':'bad\nvalue'},{'Content-Length':'0'},{'Transfer-Encoding':'chunked'},
                            {'Content-Type':'text/plain'},{'Access-Control-Request-Method':'GET'},{'Access-Control-Request-Headers':''}]:
                self.assertIsNone(archive.response({**request,'headers':headers},resource_type)[0],(kind,headers))
            for extra in [{'method':'POST'},{'method':'HEAD'},{'method':'OPTIONS'},{'postData':''},{'postDataEntries':[]},{'hasPostData':True}]:
                self.assertIsNone(archive.response({**request,**extra},resource_type)[0],(kind,extra))

    def test_version3_rejects_unproved_mime_schema_and_context_combinations(self):
        url='https://cdn.fixture.invalid/image';self.fixture.add(url,'image.bin',b'image',content_type='image/png')
        item=self.fixture.resources[-1];item.update(request_origin_absent=True,request_origin_absent_asset='image');original=json.loads(json.dumps(item))
        for schema in [1,2]:
            with self.assertRaisesRegex(replay.ReplayError,'unbound_replay_variant'):self.fixture.archive({'schema':schema})
        changes=[{'request_origin_absent_asset':value} for value in [None,False,[],{},'document','unknown']]
        changes += [{'request_origin_absent':False},{'request_origin':'https://fixture.invalid'},{'request_origin_absent_stylesheet':True},
                    {'method':'POST'},{'request_body_sha256':digest(b'body')}]
        changes += [{'headers':[{'name':'Content-Type','value':mime}]} for mime in ['text/html','text/css','image/','image/*','image/bad value']]
        changes += [{'headers':[{'name':'Content-Type','value':'image/png'},{'name':'Vary','value':'Sec-Fetch-Dest'}]}]
        for change in changes:
            item.clear();item.update(original);item.update(change)
            with self.subTest(change=change),self.assertRaises(replay.ReplayError):self.fixture.archive({'schema':3})
        item.clear();item.update(original)
        self.fixture.add(url,'other.bin',b'other',content_type='image/png');self.fixture.resources[-1].update(request_origin_absent=True,request_origin_absent_asset='image')
        with self.assertRaisesRegex(replay.ReplayError,'ambiguous_replay_resource'):self.fixture.archive({'schema':3})

    def test_version3_selects_disjoint_asset_bodies_and_does_not_admit_asset_redirect_origins(self):
        url='https://cdn.fixture.invalid/shared'
        for kind,mime,body in [('image','image/png',b'image'),('script','text/javascript',b'script')]:
            self.fixture.add(url,kind+'.bin',body,content_type=mime);self.fixture.resources[-1].update(request_origin_absent=True,request_origin_absent_asset=kind)
        self.fixture.add('https://redirect.fixture.invalid/style','redirect.bin',b'<!doctype html>',status=302,content_type='text/html')
        self.fixture.resources[-1].update(request_origin_absent=True,request_origin_absent_asset='stylesheet')
        for reverse in [False,True]:
            if reverse:self.fixture.resources[1:]=reversed(self.fixture.resources[1:])
            archive=self.fixture.archive({'schema':3});self.assertEqual(archive.document_origins,{'https://fixture.invalid'})
            for role,body in [('Image',b'image'),('Script',b'script')]:
                reply,_=archive.response({'url':url,'method':'GET','headers':{'Accept':'*/*'}},role);self.assertEqual(base64.b64decode(reply['body']),body)
        (self.fixture.root/'image.bin').write_bytes(b'changed')
        with self.assertRaisesRegex(replay.ReplayError,'replay_body_changed'):archive.response({'url':url,'method':'GET','headers':{'Accept':'*/*'}},'Image')
        self.assertIsNotNone(archive.response({'url':url,'method':'GET','headers':{'Accept':'*/*'}},'Script')[0])

    def test_anonymous_json_other_type_requires_an_actual_document_preload_declaration(self):
        url='https://cdn.fixture.invalid/data'
        html=('<!doctype html><html><head><link rel=preload as=fetch crossorigin=anonymous href="'+url+'"></head><body></body></html>').encode()
        self.fixture.add('https://fixture.invalid/declared','declaration.html',html,content_type='text/html')
        document=self.fixture.resources[-1]
        self.fixture.add(url,'data.json',b'{}',content_type='application/json');self.fixture.resources[-1].update(request_origin_absent=True,request_origin_absent_asset='json')
        archive=self.fixture.archive({'schema':3});self.assertIn(url,archive.fetch_preloads)
        request={'url':url,'method':'GET','headers':{'Accept':'*/*'}}
        self.assertIsNotNone(archive.response(request,'Other')[0])
        document['status']=302;document.update(request_origin_absent=True,request_origin_absent_asset='stylesheet')
        archive=self.fixture.archive({'schema':3});self.assertNotIn(url,archive.fetch_preloads)
        self.assertIsNone(archive.response(request,'Other')[0]);self.assertIsNotNone(archive.response(request,'Fetch')[0])
    def origin_variants(self):
        url='https://cdn.fixture.invalid/shared.css'
        headers=[{'name':'Content-Type','value':'text/css'},{'name':'Vary','value':'Origin'}]
        self.fixture.add(url,'origin.css',b'input{color:green}',headers=headers)
        self.fixture.resources[-1]['request_origin']='https://fixture.invalid'
        self.fixture.add(url,'absent.css',b'input{background:white}',headers=headers)
        self.fixture.resources[-1].update(request_origin_absent=True,request_origin_absent_stylesheet=True)
        return url

    def test_version2_selects_independent_origin_bodies_without_order_or_context_fallback(self):
        url=self.origin_variants()
        with self.assertRaisesRegex(replay.ReplayError,'ambiguous_replay_resource'):self.fixture.archive()
        for reverse in [False,True]:
            if reverse:self.fixture.resources[1:]=reversed(self.fixture.resources[1:])
            archive=self.fixture.archive({'schema':2})
            for headers,expected in [({'Origin':'https://fixture.invalid'},b'input{color:green}'),({'Accept':'text/css'},b'input{background:white}')]:
                result,reason=archive.response({'url':url,'method':'GET','headers':headers},'Stylesheet')
                self.assertEqual(reason,'recorded_response');self.assertEqual(base64.b64decode(result['body']),expected)
            for headers in [{'Origin':'https://other.invalid'},{'Origin':'https://fixture.invalid','origin':'https://fixture.invalid'},
                            {'Origin':'https://fixture.invalid','Cookie':''},{'Accept':'text/css','Authorization':'fictional'},
                            {'Accept':'text/css','Access-Control-Request-Headers':'x-unknown'}]:
                result,_=archive.response({'url':url,'method':'GET','headers':headers},'Stylesheet');self.assertIsNone(result)
            for extra in [{'postData':''},{'hasPostData':True},{'postDataEntries':[]}]:
                result,_=archive.response({'url':url,'method':'GET','headers':{'Origin':'https://fixture.invalid'},**extra},'Stylesheet');self.assertIsNone(result)
            result,_=archive.response({'url':url,'method':'GET','headers':{'Accept':'text/css'}},'Script');self.assertIsNone(result)
        (self.fixture.root/'absent.css').write_bytes(b'changed')
        result,_=archive.response({'url':url,'method':'GET','headers':{'Origin':'https://fixture.invalid'}},'Stylesheet');self.assertIsNotNone(result)
        with self.assertRaisesRegex(replay.ReplayError,'replay_body_changed'):
            archive.response({'url':url,'method':'GET','headers':{'Accept':'text/css'}},'Stylesheet')

    def test_version2_rejects_overlapping_duplicate_and_excessive_candidates(self):
        self.origin_variants();original=list(self.fixture.resources)
        for candidate in [dict(original[1]),{k:v for k,v in original[1].items() if k!='request_origin'}]:
            if 'request_origin' not in candidate:candidate['headers']=[{'name':'Content-Type','value':'text/css'}]
            self.fixture.resources=[*original,candidate]
            with self.assertRaisesRegex(replay.ReplayError,'ambiguous_replay_resource'):self.fixture.archive({'schema':2})
        self.fixture.resources=[original[0]]
        for i in range(9):
            self.fixture.resources.append({**original[1],'request_origin':'https://site'+str(i)+'.invalid'})
        with self.assertRaisesRegex(replay.ReplayError,'ambiguous_replay_resource'):self.fixture.archive({'schema':2})

    def test_version2_accept_variants_remain_documents_and_never_match_unknown_accept(self):
        url=self.fixture.resources[0]['url'];headers=[{'name':'Content-Type','value':'text/html'},{'name':'Vary','value':'Origin, Accept'}]
        self.fixture.resources[0].update(headers=headers,request_origin_absent=True,request_accept='text/html')
        self.fixture.add(url,'alternate.html',b'<!doctype html>Alternate document',headers=headers)
        self.fixture.resources[-1].update(request_origin_absent=True,request_accept='application/xhtml+xml')
        archive=self.fixture.archive({'schema':2})
        for accept in ['text/html','application/xhtml+xml']:
            result,_=archive.response({'url':url,'method':'GET','headers':{'Accept':accept}},'Document');self.assertIsNotNone(result)
        for kind,accept in [('Document','*/*'),('Other','text/html'),('XHR','text/html')]:
            result,_=archive.response({'url':url,'method':'GET','headers':{'Accept':accept}},kind);self.assertIsNone(result)

    def test_public_document_accept_has_no_header_injection_or_unbounded_media_ranges(self):
        self.assertTrue(replay.public_document_accept('text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,*/*;q=0.8,application/signed-exchange;v=b3'))
        for value in [None,True,1,[],{},'', ' text/html', 'text/html ', 'text/html,\r\nX: injected',
                      'text/html,\napplication/json','text/html,\tapplication/json','text/html,\u00a0application/json',
                      'text/html,','text/html;token','text/html;x=secret@example.invalid','a/b,'*33]:
            with self.subTest(value=value):self.assertFalse(replay.public_document_accept(value))
    def test_document_variant_requires_exact_anonymous_accept_and_absent_requested_with(self):
        item=self.fixture.resources[0]
        item.update(request_accept='*/*',request_origin_absent=True,request_x_requested_with_absent=True)
        item['headers'].append({'name':'Vary','value':'Accept-Encoding, Accept, X-Requested-With, Origin'})
        archive=self.fixture.archive()
        request={'url':item['url'],'method':'GET','headers':{'Accept':'*/*'}}
        self.assertEqual(archive.response(request,'Document')[1],'recorded_response')
        self.assertEqual(archive.resources[('GET',item['url'],digest(b''))][0]['request_accept'],'*/*')
        for headers in [{},{'Accept':'text/html'},{'Accept':' */*'},{'Accept':'*/*','accept':'*/*'},
                        {'Accept':'*/*','Cookie':'private'},{'Accept':'*/*','Authorization':'private'},
                        {'Accept':'*/*','X-Requested-With':'XMLHttpRequest'},
                        {'Accept':'*/*','Content-Length':'0'},{'Accept':'*/*','Access-Control-Request-Method':'GET'}]:
            with self.subTest(headers=headers):self.assertNotEqual(archive.response({**request,'headers':headers},'Document')[1],'recorded_response')
        for kind in [None,'Fetch','XHR','Image','Stylesheet','Script']:
            self.assertEqual(archive.response(request,kind)[1],'document_variant_not_recorded')
        self.assertNotEqual(archive.response({**request,'method':'POST'},'Document')[1],'recorded_response')
        for body in [{'hasPostData':True},{'postData':''},{'postDataEntries':[]}]:
            self.assertNotEqual(archive.response({**request,**body},'Document')[1],'recorded_response')

    def test_requested_with_absence_alone_does_not_invent_an_accept_policy(self):
        item=self.fixture.resources[0];item.update(request_origin_absent=True,request_x_requested_with_absent=True)
        item['headers'].append({'name':'Vary','value':'X-Requested-With'})
        archive=self.fixture.archive();self.assertNotIn('request_accept',archive.resources[('GET',item['url'],digest(b''))][0])
        request={'url':item['url'],'method':'GET','headers':{'Accept':'text/html'}}
        self.assertEqual(archive.response(request,'Document')[1],'recorded_response')
        for value in ['', 'XMLHttpRequest']:
            self.assertEqual(archive.response({**request,'headers':{**request['headers'],'x-requested-with':value}},'Document')[1],'document_requested_with_variant_not_recorded')

    def test_document_variant_manifest_refuses_missing_extra_or_forged_bindings(self):
        item=self.fixture.resources[0];original=dict(item)
        for binding in [{},{'request_accept':'text/html'},{'request_accept':True},{'request_accept':'*/*'},
                        {'request_accept':'*/*','request_origin_absent':True,'request_x_requested_with_absent':1},
                        {'request_accept':'*/*','request_origin_absent':True,'request_x_requested_with_absent':False}]:
            item.clear();item.update(original);item.update(binding)
            item['headers']=original['headers']+[{'name':'Vary','value':'Accept, X-Requested-With'}]
            with self.subTest(binding=binding),self.assertRaisesRegex(replay.ReplayError,'unbound_replay_variant'):self.fixture.archive()
        item.clear();item.update(original);item.update(request_accept='*/*',request_origin_absent=True)
        item['headers']=original['headers']+[{'name':'Vary','value':'Accept'}]
        for changes in [{'method':'HEAD'},{'request_body_sha256':digest(b'not empty')},
                        {'headers':[{'name':'Content-Type','value':'application/json'},{'name':'Vary','value':'Accept'}]},
                        {'headers':original['headers']},{'request_origin_absent_stylesheet':True}]:
            valid=dict(item);item.update(changes)
            with self.subTest(changes=changes),self.assertRaises(replay.ReplayError):self.fixture.archive()
            item.clear();item.update(valid)
        redirect={**item,'url':'https://fixture.invalid/redirect','status':302,
            'headers':[{'name':'Content-Type','value':'text/plain'},{'name':'Vary','value':'Accept'}]}
        self.fixture.resources.append(redirect)
        self.assertEqual(self.fixture.archive().resources[('GET',item['url'],digest(b''))][0]['request_accept'],'*/*')
    def test_origin_bound_variant_requires_get_document_and_absent_origin(self):
        item=self.fixture.resources[0]
        item['headers'].append({'name':'Vary','value':'Accept-Encoding, Origin'})
        item['request_origin_absent']=True
        archive=self.fixture.archive()
        request={'url':item['url'],'method':'GET','headers':{}}
        self.assertEqual(archive.response(request,'Document')[1],'recorded_response')
        for kind in [None,'Fetch','XHR','Image','Script']:
            self.assertEqual(archive.response(request,kind)[1],'origin_variant_not_recorded')
        for headers in [{'Origin':'null'},{'origin':''},{'ORIGIN':'https://fixture.invalid'},
                        {'Accept':'text/html','accept':'text/html'},{'bad name':'x'}]:
            self.assertEqual(archive.response({**request,'headers':headers},'Document')[1],'origin_variant_not_recorded')

    def test_origin_variant_manifest_cannot_omit_or_forge_its_binding(self):
        item=self.fixture.resources[0]
        original=list(item['headers'])
        for vary,marker in [('Origin',None),('Origin',False),('Origin',1),('*',True),('Origin, Accept',True),('Accept-Encoding',True)]:
            item['headers']=original+[{'name':'Vary','value':vary}]
            item.pop('request_origin_absent',None)
            if marker is not None:item['request_origin_absent']=marker
            with self.subTest(vary=vary,marker=marker),self.assertRaisesRegex(replay.ReplayError,'unbound_replay_variant'):
                self.fixture.archive()
    def test_explicit_public_origin_binds_even_a_response_without_vary(self):
        url='https://cdn.fixture.invalid/app.js'
        self.fixture.add(url,'app.bin',b'void 0',content_type='text/javascript')
        self.fixture.resources[-1]['request_origin']='https://fixture.invalid'
        archive=self.fixture.archive()
        request={'url':url,'method':'GET','headers':{'Origin':'https://fixture.invalid'}}
        self.assertEqual(archive.response(request,'Script')[1],'recorded_response')
        for headers in [{},{'Origin':'null'},{'Origin':'https://other.invalid'},
                        {'Origin':'https://fixture.invalid','origin':'https://fixture.invalid'}]:
            self.assertEqual(archive.response({**request,'headers':headers},'Script')[1],'origin_variant_not_recorded')
        self.fixture.resources[-1]['headers'].append({'name':'Vary','value':'Origin'})
        self.assertEqual(self.fixture.archive().response(request,'Script')[1],'recorded_response')
        self.fixture.resources[-1]['request_origin_absent']=True
        with self.assertRaisesRegex(replay.ReplayError,'unbound_replay_variant'):self.fixture.archive()

    def test_absent_origin_stylesheet_replays_only_observed_anonymous_style_gets(self):
        url='https://cdn.fixture.invalid/style.css'
        self.fixture.add(url,'style.css',b'body{color:navy}',content_type='text/css')
        item=self.fixture.resources[-1]
        item.update(request_origin_absent=True,request_origin_absent_stylesheet=True)
        request={'url':url,'method':'GET','headers':{'Accept':'text/css,*/*;q=0.1'}}
        for vary in [None,'Accept-Encoding, Origin']:
            item['headers']=[{'name':'Content-Type','value':'text/css'}]
            if vary:item['headers'].append({'name':'Vary','value':vary})
            archive=self.fixture.archive()
            self.assertEqual(archive.response(request,'Stylesheet')[1],'recorded_response')
            for kind in [None,'Document','Fetch','XHR','Script','Image','Other']:
                with self.subTest(kind=kind,vary=vary):
                    self.assertIsNone(archive.response(request,kind)[0])
            # Even empty credentials or Origin are explicit, incompatible evidence.
            variants=[{},None,[],{'Accept':'text/css','accept':'text/css'},{'bad name':'x'},
                      {'Accept':7},{'Accept':'text/css\r\nOrigin: forged'}]
            variants += [{**request['headers'],name:''} for name in ['Origin','Cookie','Authorization',
                         'Proxy-Authorization','Access-Control-Request-Headers','Access-Control-Request-Method']]
            for headers in variants:
                with self.subTest(headers=headers,vary=vary):
                    self.assertIsNone(archive.response({**request,'headers':headers},'Stylesheet')[0])
            missing={k:v for k,v in request.items() if k!='headers'}
            self.assertIsNone(archive.response(missing,'Stylesheet')[0])
            for changes in [{'method':'POST'},{'method':'HEAD'},{'hasPostData':True},
                            {'postData':''},{'postDataEntries':[]}]:
                with self.subTest(changes=changes):
                    self.assertIsNone(archive.response({**request,**changes},'Stylesheet')[0])

    def test_absent_origin_stylesheet_manifest_cannot_broaden_scope(self):
        url='https://cdn.fixture.invalid/style.css'
        self.fixture.add(url,'style.css',b'body{}',content_type='text/css')
        item=self.fixture.resources[-1]
        original={**item,'request_origin_absent':True,'request_origin_absent_stylesheet':True}
        invalid=[{'request_origin_absent_stylesheet':False},{'request_origin_absent_stylesheet':1},
                 {'request_origin_absent':False},{'request_origin_absent':None},
                 {'request_origin':'https://fixture.invalid'},{'method':'POST'},
                 {'headers':[{'name':'Content-Type','value':'application/json'}]},
                 {'headers':[{'name':'Content-Type','value':'text/css'},{'name':'Vary','value':'*'}]}]
        for changes in invalid:
            item.clear();item.update({**original,**changes})
            with self.subTest(changes=changes),self.assertRaises(replay.ReplayError):self.fixture.archive()
        item.clear();item.update(original);item.pop('request_origin_absent')
        with self.assertRaises(replay.ReplayError):self.fixture.archive()
        item.clear();item.update(original);item['status']=302
        item['headers']=[{'name':'Location','value':'/next.css'},{'name':'Vary','value':'Origin'}]
        archive=self.fixture.archive()
        request={'url':url,'method':'GET','headers':{'Accept':'text/css'}}
        self.assertEqual(archive.response(request,'Stylesheet')[1],'recorded_response')
        self.assertIsNone(archive.response(request,'Document')[0])

    def test_preflight_absence_requires_manifest_proof_and_matching_bodyless_get(self):
        url='https://cdn.fixture.invalid/bootstrap'
        self.fixture.add(url,'bootstrap.bin',b'{}',content_type='application/json')
        item=self.fixture.resources[-1]
        item['headers'].append({'name':'Vary','value':'Origin, Access-Control-Request-Headers, Access-Control-Request-Method'})
        item['request_origin']='https://fixture.invalid'
        request={'url':url,'method':'GET','headers':{'Origin':'https://fixture.invalid'}}
        for marker in [None,False,1]:
            item.pop('request_preflight_absent',None)
            if marker is not None:item['request_preflight_absent']=marker
            with self.assertRaisesRegex(replay.ReplayError,'unbound_replay_variant'):self.fixture.archive()
        item['request_preflight_absent']=True
        archive=self.fixture.archive()
        self.assertEqual(archive.response(request,'Fetch')[1],'recorded_response')
        for name in ['Access-Control-Request-Headers','access-control-request-method']:
            changed={**request,'headers':{**request['headers'],name:''}}
            self.assertEqual(archive.response(changed,'Fetch')[1],'preflight_variant_not_recorded')
        for changes in [{'method':'OPTIONS'},{'method':'POST'},{'hasPostData':True}]:
            self.assertIsNone(archive.response({**request,**changes},'Fetch')[0])
        item['headers']=[{'name':'Content-Type','value':'application/json'}]
        with self.assertRaisesRegex(replay.ReplayError,'unbound_replay_variant'):self.fixture.archive()

    def test_large_script_replays_with_the_request_and_aggregate_budgets_preserved(self):
        url='https://cdn.fixture.invalid/app.js'
        data=b'/*'+b'x'*(4*1024**2)+b'*/'
        self.fixture.add(url,'app.bin',data,content_type='text/javascript')
        archive=self.fixture.archive()
        response,reason=archive.response({'url':url,'method':'GET'},'Script')
        self.assertEqual(reason,'recorded_response')
        self.assertEqual(digest(base64.b64decode(response['body'])),digest(data))
        self.assertEqual(archive.response({'url':url,'method':'POST','postData':'x'*(4*1024**2+1)})[1],'opaque_request_body')
        self.fixture.add(url+'2','oversize.bin',b'x'*(replay.MAX_BODY+1),content_type='text/javascript')
        with self.assertRaises(replay.ReplayError):self.fixture.archive()

    def test_legacy_document_origin_ineligible_for_collection_still_replays(self):
        for url in ['https://[2606:4700:4700::1111]/form','https://fixture.invalid:443/form']:
            self.fixture.resources[0]['url']=url
            archive=self.fixture.archive({'entry_url':url})
            self.assertEqual(archive.response({'url':url,'method':'GET'},'Document')[1],'recorded_response')
            self.assertEqual(archive.document_origins,set())

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
        with patch.object(replay,'MAX_REQUEST_BODY',64):
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

    def test_refusal_context_is_bounded_redacted_and_not_acknowledged_delivery(self):
        self.bind()
        url='https://fixture.invalid/absent?email=fictional@example.invalid'
        request={'url':url,'method':'POST','headers':{'Origin':'https://private.invalid',
                 'Authorization':'Bearer fictional-secret','Access-Control-Request-Method':'POST'},
                 'postData':'candidate-secret'}
        event={'method':'Fetch.requestPaused','sessionId':'page','params':{
            'requestId':'refused','resourceType':'XHR','request':request}}
        self.broker.browser(event)
        sample=self.broker.report()['refused_request_samples'][0]
        self.assertEqual(sample,dict(url_sha256=digest(url.encode()),method='POST',resource_type='XHR',
            reason='response_not_recorded',origin_state='present',preflight_headers_present=True,observations=1))
        self.assertEqual(self.broker.report()['blocked'],0)
        self.assertEqual(self.sent[-1]['method'],'Fetch.failRequest');self.ack()
        self.broker.browser(event);self.ack()
        self.assertEqual(self.broker.report()['refused_request_samples'][0]['observations'],2)
        self.assertEqual(self.broker.report()['blocked'],2)
        for index in range(35):
            event['params']['request']={**request,'url':url+'&case='+str(index)}
            self.broker.browser(event);self.ack()
        report=self.broker.report()
        self.assertEqual(len(report['refused_request_samples']),32)
        self.assertEqual(report['omitted_refusal_samples'],4)
        self.assertEqual(report['blocked'],37)
        for secret in ['fictional@example.invalid','private.invalid','fictional-secret','candidate-secret',url]:
            self.assertNotIn(secret,json.dumps(report))
        self.broker.observe_refusal({'url':'bad','method':['private-method'],'headers':[]},['private-kind'],'unsupported_url')
        self.assertEqual(self.broker.report()['omitted_refusal_samples'],5)
        other=replay.Broker(self.fixture.archive(),self.sent.append,self.emitted.append)
        other.observe_refusal({'url':'bad','method':{},'headers':{'Origin':7,'origin':'secret'}},{},'unsupported_url')
        self.assertEqual(other.report()['refused_request_samples'],[dict(url_sha256=None,method='Other',
            resource_type='Other',reason='unsupported_url',origin_state='malformed',
            preflight_headers_present=False,observations=1)])

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

    def test_replaced_frame_churn_reuses_live_session_budget(self):
        self.bind()
        for index in range(80):
            session='frame-'+str(index)
            event={'method':'Target.attachedToTarget','sessionId':'page','params':{'sessionId':session,'waitingForDebugger':True,'targetInfo':{'type':'iframe','targetId':'target-'+str(index)}}}
            self.broker.browser(event);self.ack();self.ack();self.ack()
            self.assertTrue(self.broker.browser({'method':'Target.detachedFromTarget','sessionId':'page','params':{'sessionId':session}}))
        self.assertEqual(set(self.broker.sessions),{'page'})
        self.assertEqual(len(self.broker.targets),1)

    def test_detach_during_initialization_fails_waiters_and_consumes_late_ack(self):
        self.broker.host({'id':1,'method':'Target.attachToTarget','params':{'targetId':'root'}})
        self.broker.browser({'id':1,'result':{'sessionId':'page'}})
        pending=self.sent[-1]
        self.broker.host({'id':2,'method':'Target.attachToTarget','params':{'targetId':'root'}})
        self.broker.browser({'id':2,'result':{'sessionId':'alias'}})
        self.broker.browser({'method':'Target.detachedFromTarget','params':{'sessionId':'page'}})
        self.assertEqual({r['id'] for r in self.emitted},{1,2})
        self.assertTrue(all('error' in r and 'result' not in r for r in self.emitted))
        self.assertFalse(self.broker.browser({'id':pending['id'],'result':{}}))
        self.assertTrue(all(c['method']!='Runtime.runIfWaitingForDebugger' for c in self.sent),'detached target must not be resumed')
        cleanup=self.sent[-1];self.assertEqual(cleanup['method'],'Target.detachFromTarget');self.ack()
        self.assertEqual(self.broker.report()['closed_aliases'],1)
        self.assertEqual(self.broker.report()['discarded_replies'],1)
        self.assertEqual(self.broker.sessions,{})
        self.assertFalse(self.broker.host({'id':3,'method':'Runtime.evaluate','sessionId':'alias','params':{}}))
        self.assertIn('error',self.emitted[-1]);self.assertEqual(self.sent[-1],cleanup)

    def test_alias_detachment_preserves_the_primary_interceptor(self):
        self.bind()
        self.broker.host({'id':2,'method':'Target.attachToTarget','params':{'targetId':'root'}})
        self.broker.browser({'id':2,'result':{'sessionId':'alias'}})
        self.broker.browser({'method':'Target.detachedFromTarget','params':{'sessionId':'alias'}})
        self.assertEqual(set(self.broker.sessions),{'page'})
        self.broker.browser({'method':'Fetch.requestPaused','sessionId':'page','params':{'requestId':'r','request':{'url':'https://fixture.invalid/form','method':'GET'}}})
        self.assertEqual(self.sent[-1]['method'],'Fetch.fulfillRequest');self.ack()
        self.assertEqual(self.broker.report()['served'],1)

    def test_parent_detachment_retires_nested_targets_and_their_aliases(self):
        self.bind()
        for session,parent in [('child','page'),('nested','child')]:
            self.broker.browser({'method':'Target.attachedToTarget','sessionId':parent,'params':{'sessionId':session,'waitingForDebugger':True,'targetInfo':{'type':'iframe','targetId':session+'-target'}}})
            self.ack();self.ack();self.ack()
        self.broker.host({'id':2,'method':'Target.attachToTarget','params':{'targetId':'nested-target'}})
        self.broker.browser({'id':2,'result':{'sessionId':'alias'}})
        self.broker.browser({'method':'Target.detachedFromTarget','sessionId':'page','params':{'sessionId':'child'}})
        self.assertEqual(set(self.broker.sessions),{'page'})
        self.assertEqual(set(self.broker.session_targets),{'page'})
        self.assertEqual(set(self.broker.session_parents),{'page'})
        self.assertEqual(len(self.broker.targets),1)
        self.assertFalse(self.broker.browser({'method':'Target.detachedFromTarget','params':{'sessionId':'nested'}}))
        self.assertFalse(self.broker.browser({'id':7,'sessionId':'alias','result':{'private':'fictional private value'}}))
        self.assertNotIn('fictional private value',json.dumps(self.emitted))
        self.assertIn('error',self.emitted[-1])

    def test_detached_response_ack_does_not_count_as_success(self):
        self.bind()
        self.broker.browser({'method':'Fetch.requestPaused','sessionId':'page','params':{'requestId':'r','request':{'url':'https://fixture.invalid/form','method':'GET'}}})
        pending=self.sent[-1]
        self.broker.browser({'method':'Target.detachedFromTarget','params':{'sessionId':'page'}})
        self.broker.browser({'id':pending['id'],'result':{}})
        self.assertEqual(self.broker.report()['served'],0)
        self.assertEqual(self.broker.report()['retired_commands'],0)
        self.assertEqual(self.broker.report()['discarded_replies'],1)
        self.assertFalse(self.broker.browser({'method':'Fetch.requestPaused','sessionId':'page','params':{'private':'do not emit'}}))
        self.assertNotIn('do not emit',json.dumps(self.emitted))

    def test_detached_outline_returns_unknown_once_and_discards_late_values(self):
        self.bind();self.broker.discovery=True
        self.broker.host({'id':8,'method':'SmartJobseeker.publicFormOutline','sessionId':'page','params':{}})
        pending=self.sent[-1]
        self.broker.browser({'method':'Target.detachedFromTarget','params':{'sessionId':'page'}})
        replies=[r for r in self.emitted if r.get('id')==8]
        self.assertEqual(len(replies),1)
        self.assertEqual(replies[0]['result']['status'],'unavailable')
        self.assertEqual(replies[0]['result']['reason'],'browser_target_detached')
        self.broker.browser({'id':pending['id'],'result':{'nodes':[{'private':'fictional private value'}]}})
        self.assertEqual(len([r for r in self.emitted if r.get('id')==8]),1)
        self.assertNotIn('fictional private value',json.dumps(self.emitted))

    def test_lifecycle_history_is_bounded_and_retired_ids_cannot_be_rebound(self):
        self.bind();self.broker.browser({'method':'Target.detachedFromTarget','params':{'sessionId':'page'}})
        with self.assertRaisesRegex(replay.ReplayError,'invalid_replay_session'):
            self.broker.bind('page','page',None,False,'new-target')
        self.broker.bind('new-page','page',None,False,'new-target');self.ack();self.ack()
        with patch.object(replay,'MAX_RETIRED_SESSIONS',1), self.assertRaisesRegex(replay.ReplayError,'lifecycle_budget'):
            self.broker.browser({'method':'Target.detachedFromTarget','params':{'sessionId':'new-page'}})
        self.assertIn('new-page',self.broker.sessions)
        self.broker.browser({'method':'Fetch.requestPaused','sessionId':'new-page','params':{'requestId':'r','request':{'url':'https://fixture.invalid/form','method':'GET'}}})
        with patch.object(replay,'MAX_RETIRED_REPLIES',0), self.assertRaisesRegex(replay.ReplayError,'lifecycle_budget'):
            self.broker.browser({'method':'Target.detachedFromTarget','params':{'sessionId':'new-page'}})

    def test_reattach_uses_a_fresh_interceptor_before_releasing_the_new_session(self):
        self.broker.host({'id':1,'method':'Target.attachToTarget','params':{'targetId':'root'}})
        self.broker.browser({'id':1,'result':{'sessionId':'old'}})
        old=self.sent[-1]
        self.broker.browser({'method':'Target.detachedFromTarget','params':{'sessionId':'old'}})
        self.broker.host({'id':2,'method':'Target.attachToTarget','params':{'targetId':'root'}})
        reply={'id':2,'result':{'sessionId':'new'}}
        self.broker.browser(reply);fresh=self.sent[-1]
        self.assertEqual(fresh['method'],'Fetch.enable');self.assertEqual(fresh['sessionId'],'new')
        self.assertNotIn(reply,self.emitted)
        self.broker.browser({'id':old['id'],'error':{'message':'old target closed'}})
        self.assertEqual(self.sent[-1],fresh);self.assertNotIn(reply,self.emitted)
        self.ack();self.ack();self.assertIn(reply,self.emitted)
        self.assertEqual(set(self.broker.sessions),{'new'})

    def test_pending_parent_attach_cannot_reintroduce_a_detached_scope(self):
        self.bind()
        self.broker.host({'id':2,'method':'Target.attachToTarget','sessionId':'page','params':{'targetId':'nested'}})
        self.broker.browser({'method':'Target.detachedFromTarget','params':{'sessionId':'page'}})
        reply={'id':2,'result':{'sessionId':'late'}}
        self.assertFalse(self.broker.browser(reply))
        self.assertNotIn(reply,self.emitted)
        self.assertEqual(self.broker.sessions,{})
        self.assertEqual(self.broker.attaching,{})
        self.assertIn('error',self.emitted[-1])

    def test_already_closed_alias_is_idempotent_but_unknown_cleanup_errors_fail(self):
        self.bind()
        self.broker.host({'id':2,'method':'Target.attachToTarget','params':{'targetId':'root'}})
        self.broker.browser({'id':2,'result':{'sessionId':'alias'}})
        self.broker.browser({'method':'Target.detachedFromTarget','params':{'sessionId':'page'}})
        command=self.sent[-1]
        self.assertEqual(command['method'],'Target.detachFromTarget')
        self.broker.browser({'id':command['id'],'error':{'code':-32000,'message':'No session with given id'}})
        self.assertEqual(self.broker.report()['closed_aliases'],1)
        self.assertEqual(self.broker.report()['pending_detachments'],0)
        self.broker.bind('other','page',None,False,'other-target');self.ack();self.ack()
        self.broker.host({'id':3,'method':'Target.attachToTarget','params':{'targetId':'other-target'}})
        self.broker.browser({'id':3,'result':{'sessionId':'other-alias'}})
        self.broker.browser({'method':'Target.detachedFromTarget','params':{'sessionId':'other'}})
        command=self.sent[-1]
        with self.assertRaisesRegex(replay.ReplayError,'alias_cleanup_failed'):
            self.broker.browser({'id':command['id'],'error':{'code':-32000,'message':'different failure'}})
        self.assertEqual(self.broker.report()['closed_aliases'],1)


    def test_interception_error_override_duplicate_ids_and_unknown_sessions_fail(self):
        for method in ['Fetch.disable','Fetch.continueRequest','Fetch.enable','Target.setAutoAttach','Network.setExtraHTTPHeaders']:
            with self.assertRaises(replay.ReplayError):self.broker.host({'id':1,'method':method})
        with self.assertRaises(replay.ReplayError):self.broker.host({'id':replay.INTERNAL_ID,'method':'Page.enable'})
        self.broker.host({'id':1,'method':'Target.attachToTarget','params':{'targetId':'root'}});self.broker.browser({'id':1,'result':{'sessionId':'page'}})
        with self.assertRaisesRegex(replay.ReplayError,'interception_failed'):self.ack('Fetch refused')
        with self.assertRaises(replay.ReplayError):self.broker.browser({'id':replay.INTERNAL_ID+99,'result':{}})
        with self.assertRaises(replay.ReplayError):self.broker.browser({'method':'Fetch.requestPaused','sessionId':'unknown','params':{'requestId':'x','request':{}}})


class DiscoveryTests(unittest.TestCase):
    def setUp(self):
        self.fixture=Fixture();self.sent=[];self.emitted=[]
        self.broker=replay.Broker(self.fixture.archive(),self.sent.append,self.emitted.append,discovery=True)
        self.broker.sessions['page']='page'
    def tearDown(self):self.fixture.close()
    def frame(self,*,frame_id='root',resource_type='Other',**request):
        self.broker.browser({'method':'Fetch.requestPaused','sessionId':'page','params':{
            'requestId':'r','request':request,'frameId':frame_id,'resourceType':resource_type}})
        command=self.sent[-1]
        self.broker.browser({'id':command['id'],'result':{}})
        return command
    def report(self):
        self.assertFalse(self.broker.host({'id':99,'method':'SmartJobseeker.discoveryReport','params':{}}))
        return self.emitted[-1]['result']
    def test_anonymous_asset_contexts_are_typed_bounded_and_never_inferred_from_malformed_headers(self):
        self.broker.host({'id':1,'method':'Page.navigate','sessionId':'page','params':{'url':self.fixture.archive().entry_url}})
        for resource_type,kind in [('Image','image'),('Font','font'),('Script','script'),('Fetch','json'),('Manifest','manifest')]:
            url='https://cdn.fixture.invalid/'+kind
            self.frame(url=url,method='GET',resource_type=resource_type,headers={'Accept':'*/*'})
            self.assertEqual(self.report()['asset_origin_absent_assets'][url],kind)
            for index,headers in enumerate([{}, {'Accept':'bad\nvalue'}, {'Accept':'*/*','accept':'*/*'}, {'Content-Length':'0'},{'Origin':'null'}]):
                bad=url+str(index);self.frame(url=bad,method='GET',resource_type=resource_type,headers=headers)
                self.assertNotIn(bad,self.report()['asset_origin_absent_assets'])
        self.assertEqual(self.report()['schema'],3)
        for index,headers in enumerate([{'Cookie':''},{'Authorization':''},{'Proxy-Authorization':''}]):
            url='https://cdn.fixture.invalid/private'+str(index);self.frame(url=url,method='GET',resource_type='Image',headers=headers);self.assertNotIn(url,self.report()['urls'])
        url='https://cdn.fixture.invalid/image'
        self.frame(url=url,method='GET',resource_type='Font',headers={'Accept':'*/*'})
        self.assertNotIn(url,self.report()['asset_origin_absent_assets']);self.assertEqual(len(self.report()['asset_request_variants'][url]),2)
        self.frame(url=url,method='GET',resource_type='Media',headers={'Accept':'*/*'})
        self.assertNotIn(url,self.report()['urls']);self.assertNotIn(url,self.report()['asset_request_variants'])

    def test_anonymous_asset_proposals_share_the_existing_total_budget(self):
        self.broker.host({'id':1,'method':'Page.navigate','sessionId':'page','params':{'url':self.fixture.archive().entry_url}})
        for index in range(140):self.frame(url='https://cdn.fixture.invalid/'+str(index),method='GET',resource_type='Image',headers={'Accept':'*/*'})
        value=self.report();self.assertEqual(len(value['urls']),128);self.assertEqual(len(value['asset_origin_absent_assets']),128);self.assertEqual(value['omitted_requests'],12)
    def test_initial_document_accept_observation_is_exact_anonymous_and_single_use(self):
        self.frame(resource_type='Document',url='https://fixture.invalid/form',method='GET',headers={'Accept':'text/html,application/xhtml+xml;q=0.9'})
        report=self.report();self.assertEqual(report['entry_document_requests'],1)
        self.assertEqual(report['entry_request_accept'],'text/html,application/xhtml+xml;q=0.9')
        self.frame(resource_type='Document',url='https://fixture.invalid/form',method='GET',headers={'Accept':'text/html,application/xhtml+xml;q=0.9'})
        report=self.report();self.assertEqual(report['entry_document_requests'],2);self.assertIsNone(report['entry_request_accept'])

    def test_document_request_probe_never_uses_sensitive_body_or_ambiguous_header_context(self):
        for changes in [
            {'headers':{}},{'headers':{'Accept':'text/html','accept':'text/html'}},
            {'headers':{'Accept':'text/html,\napplication/json'}},
            {'headers':{'Accept':'text/html','Cookie':'fictional-secret'}},
            {'headers':{'Accept':'text/html','Origin':'https://fixture.invalid'}},
            {'headers':{'Accept':'text/html','Content-Length':'0'}},
            {'headers':{'Accept':'text/html','Access-Control-Request-Method':'GET'}},
            {'hasPostData':True},{'postData':''},{'postDataEntries':[]},
        ]:
            self.broker=replay.Broker(self.fixture.archive(),self.sent.append,self.emitted.append,discovery=True);self.broker.sessions['page']='page'
            request={'url':'https://fixture.invalid/form','method':'GET','headers':{'Accept':'text/html'},**changes}
            self.frame(resource_type='Document',**request);report=self.report()
            self.assertIsNone(report['entry_request_accept']);self.assertNotIn('fictional-secret',json.dumps(report))
    def outline(self, nodes):
        self.assertFalse(self.broker.host({'id': 81, 'method': 'SmartJobseeker.publicFormOutline', 'sessionId': 'page', 'params': {}}))
        command = self.sent[-1]
        self.assertEqual(command['method'], 'Accessibility.getFullAXTree')
        self.assertEqual(command['params'], {})
        self.assertFalse(self.broker.browser({'id':command['id'], 'result':{'nodes':nodes}}))
        return self.emitted[-1]['result']
    def test_public_outline_keeps_context_and_unknowns_but_no_values_or_handles(self):
        nodes = [
            {'nodeId':'root','ignored':False,'role':{'value':'RootWebArea'}},
            {'nodeId':'group','parentId':'root','ignored':False,'role':{'value':'group'},'name':{'value':'Applicant details'}},
            {'nodeId':'field','parentId':'group','ignored':False,'role':{'value':'textbox'},'name':{'value':'Full name'},
             'value':{'value':'private-marker-not-a-label'},'backendDOMNodeId':123456,
             'properties':[{'name':'required','value':{'value':True}},{'name':'checked','value':{'value':'private-choice'}}]},
        ]
        result = self.outline(nodes)
        self.assertEqual(result['status'],'observed')
        self.assertEqual(result['controls'][0]['context'],['Applicant details'])
        self.assertEqual(result['controls'][0]['label'],'Full name')
        self.assertIs(result['controls'][0]['required'],True)
        self.assertIsNone(result['controls'][0]['disabled'])
        self.assertNotIn('private-',json.dumps(result))
        self.assertNotIn('backendDOM',json.dumps(result))
        self.assertNotIn('123456',json.dumps(result))
        self.assertEqual(self.report()['form_outline'],result)
        self.assertNotIn('form_outline',self.broker.report())
    def test_outline_cannot_expand_the_discovery_protocol_or_be_repeated(self):
        for method,params in [('Accessibility.getFullAXTree',{}),('SmartJobseeker.publicFormOutline',{'candidate':'private'})]:
            with self.assertRaisesRegex(replay.ReplayError,'discovery_command_refused'):
                self.broker.host({'id':1,'method':method,'sessionId':'page','params':params})
        self.assertEqual(self.sent,[])
        self.outline([{'nodeId':'1','ignored':False,'role':{'value':'RootWebArea'}}])
        with self.assertRaisesRegex(replay.ReplayError,'outline_session_or_budget_refused'):
            self.broker.host({'id':2,'method':'SmartJobseeker.publicFormOutline','sessionId':'page','params':{}})
        regular = replay.Broker(self.fixture.archive(),self.sent.append,self.emitted.append)
        with self.assertRaisesRegex(replay.ReplayError,'not_resource_discovery'):
            regular.host({'id':3,'method':'SmartJobseeker.publicFormOutline','sessionId':'page','params':{}})
    def test_raw_accessibility_updates_and_read_errors_do_not_leak_values(self):
        self.assertFalse(self.broker.browser({'method':'Accessibility.nodesUpdated','params':{'nodes':[{'value':'private-marker'}]}}))
        self.assertEqual(self.emitted,[])
        self.broker.host({'id':1,'method':'SmartJobseeker.publicFormOutline','sessionId':'page','params':{}})
        self.assertFalse(self.broker.browser({'id':self.sent[-1]['id'],'error':{'message':'private-marker'}}))
        result=self.emitted[-1]['result']
        self.assertEqual(result['status'],'unavailable')
        self.assertNotIn('private-marker',json.dumps(result))
    def test_outline_bounds_are_explicit_and_do_not_truncate_questions(self):
        nodes=[{'nodeId':str(i),'ignored':False,'role':{'value':'textbox'},'name':{'value':'Question '+str(i)}} for i in range(257)]
        result=replay.public_form_outline({'nodes':nodes})
        self.assertEqual(result['status'],'partial')
        self.assertEqual(len(result['controls']),256)
        self.assertEqual(result['omitted_controls'],1)
        nodes[0]['name']['value']='問'*200
        result=replay.public_form_outline({'nodes':nodes[:1]})
        self.assertIsNone(result['controls'][0]['label'])
        self.assertEqual(result['omitted_texts'],1)
        self.assertEqual(result['unlabelled_controls'],1)
        self.assertEqual(replay.public_form_outline({'nodes':nodes*64})['status'],'unavailable')
    def test_conflicting_flags_and_ancestry_cycles_stay_unknown(self):
        node={'nodeId':'1','parentId':'1','ignored':False,'role':{'value':'checkbox'},'name':{'value':'Yes'},
              'properties':[{'name':'required','value':{'value':True}},{'name':'required','value':{'value':False}}]}
        result=replay.public_form_outline({'nodes':[node]})
        self.assertEqual(result['status'],'partial')
        self.assertIsNone(result['controls'][0]['required'])
        self.assertTrue(result['controls'][0]['context_incomplete'])
    def test_only_anonymous_bodyless_missing_gets_become_sorted_proposals(self):
        for path in ['/z.js','/a.css','/z.js']:
            command=self.frame(url='https://fixture.invalid'+path,method='GET')
            self.assertEqual(command['method'],'Fetch.failRequest')
        report=self.report();self.assertEqual(report['urls'],['https://fixture.invalid/a.css','https://fixture.invalid/z.js'])
        self.assertEqual(report['blocked'],3);self.assertEqual(report['pending_commands'],0)
        self.assertNotIn('urls',self.broker.report())
    def test_authentication_cookies_and_nonread_or_opaque_bodies_are_not_proposed(self):
        for extra in [{'method':'POST'},{'method':'HEAD'},{'method':'GET','hasPostData':True},
                      {'method':'GET','postData':'candidate'}, {'method':'GET','headers':{'Authorization':'private'}},
                      {'method':'GET','headers':{'Cookie':'private'}},{'method':'GET','headers':[]},
                      {'method':'GET','url':'http://fixture.invalid/x'}]:
            self.frame(**{'url':'https://fixture.invalid/missing','method':'GET',**extra})
        report=self.report();self.assertEqual(report['urls'],[]);self.assertEqual(report['ineligible_requests'],8)
        self.assertNotIn('candidate',json.dumps(report));self.assertNotIn('private',json.dumps(report))
    def test_candidate_entry_and_protocol_escape_commands_are_refused_before_send(self):
        commands=[('Input.insertText',{'text':'candidate'}),('Input.dispatchMouseEvent',{}),
                  ('DOM.setFileInputFiles',{'files':['private.pdf']}),('Runtime.evaluate',{'expression':'document.forms[0].submit()'}),
                  ('Runtime.callFunctionOn',{}),('Page.setDocumentContent',{}),('Network.setExtraHTTPHeaders',{}),
                  ('Page.navigate',{'url':'https://fixture.invalid/submit'}),('Target.createTarget',{'url':'https://fixture.invalid/submit'}),
                  ('Target.createTarget',{'url':'about:blank','browserContextId':'other'}),
                  ('CSS.createStyleSheet',{'frameId':'root','force':True}),
                  ('CSS.setStyleSheetText',{'styleSheetId':'source','text':'body{display:none}'}),
                  ('CSS.enable',{'unknown':True}),('DOM.enable',{'includeWhitespace':'all'})]
        for method,params in commands:
            with self.subTest(method=method),self.assertRaisesRegex(replay.ReplayError,'discovery_command_refused'):
                self.broker.host({'id':1,'method':method,'params':params})
        self.assertEqual(self.sent,[])
    def test_navigation_is_bound_to_the_recorded_entry_and_empty_domain_enables(self):
        for method,params in [('Page.enable',{}),('Network.enable',{}),('DOM.enable',{}),('CSS.enable',{}),('Browser.getVersion',{}),
                              ('Target.createTarget',{'url':'about:blank'})]:
            self.assertTrue(self.broker.host({'id':1,'method':method,'params':params}))
        navigation={'id':2,'method':'Page.navigate','params':{'url':self.fixture.archive().entry_url}}
        with self.assertRaisesRegex(replay.ReplayError,'discovery_navigation_scope_refused'):
            self.broker.host(navigation)
        self.assertTrue(self.broker.host({**navigation,'sessionId':'page'}))
        with self.assertRaisesRegex(replay.ReplayError,'discovery_navigation_scope_refused'):
            self.broker.host({**navigation,'sessionId':'page'})
        with self.assertRaises(replay.ReplayError):self.broker.host({'id':1,'method':'Page.enable','params':{'unknown':True}})
    def test_child_document_proposals_require_observed_frame_and_anonymous_get(self):
        self.assertTrue(self.broker.host({'id':1,'method':'Page.navigate','sessionId':'page',
                                         'params':{'url':self.fixture.archive().entry_url}}))
        self.broker.browser({'method':'Page.frameAttached','sessionId':'page',
                             'params':{'frameId':'child','parentFrameId':'root'}})
        origin='https://different.fixture.invalid'
        self.frame(url=origin+'/frame',method='GET')
        self.frame(url=origin+'/frame',method='GET',frame_id='child',resource_type='Document')
        self.frame(url=origin+'/script',method='GET',frame_id='child',resource_type='Script')
        self.frame(url=origin+'/top',method='GET',resource_type='Document')
        self.frame(url=origin+'/private',method='GET',frame_id='child',resource_type='Document',
                   headers={'Authorization':'fictional'})
        self.broker.browser({'method':'Page.frameDetached','sessionId':'page','params':{'frameId':'child'}})
        self.frame(url=origin+'/stale',method='GET',frame_id='child',resource_type='Document')
        result=self.report()
        self.assertEqual(result['child_documents'],[origin+'/frame'])
        self.assertEqual(result['ineligible_requests'],1)
        self.assertEqual(result['blocked'],6)
    def test_typed_assets_require_the_owned_discovery_context_and_anonymous_get(self):
        url='https://cdn.fixture.invalid/app.js'
        self.frame(url=url,method='GET',resource_type='Script')
        self.assertEqual(self.report()['assets'],{})
        self.broker.host({'id':1,'method':'Page.navigate','sessionId':'page','params':{'url':self.fixture.archive().entry_url}})
        self.frame(url=url,method='GET',resource_type='Script')
        self.frame(url='https://cdn.fixture.invalid/style',method='GET',resource_type='Stylesheet')
        self.frame(url='https://cdn.fixture.invalid/manifest',method='GET',resource_type='Fetch')
        for label,changes in [('write',{'method':'POST'}),('auth',{'headers':{'Authorization':'fictional'}}),
                              ('cookie',{'headers':{'Cookie':'fictional=private'}}),('body',{'postData':''})]:
            self.frame(url='https://cdn.fixture.invalid/'+label,method=changes.pop('method','GET'),resource_type='Script',**changes)
        self.frame(url='https://cdn.fixture.invalid/document',method='GET',resource_type='Document')
        self.broker.sessions['other']='page'
        self.broker.browser({'method':'Fetch.requestPaused','sessionId':'other','params':{'requestId':'other',
            'resourceType':'Script','request':{'url':'https://unrelated.fixture.invalid/app.js','method':'GET'}}})
        report=self.report()
        self.assertEqual(report['assets'],{url:['script'],'https://cdn.fixture.invalid/style':['stylesheet'],
                                          'https://cdn.fixture.invalid/manifest':['json']})
        self.assertEqual(report['ineligible_requests'],4)
    def test_only_admitted_document_origin_is_projected_and_conflicting_variants_are_removed(self):
        url='https://cdn.fixture.invalid/app.js'
        self.broker.host({'id':1,'method':'Page.navigate','sessionId':'page','params':{'url':self.fixture.archive().entry_url}})
        self.frame(url=url,method='GET',resource_type='Script',headers={'Origin':'https://fixture.invalid'})
        self.assertEqual(self.report()['asset_request_origins'],{url:'https://fixture.invalid'})
        self.assertNotIn('asset_request_origins',self.broker.report())
        for index,origin in enumerate(['null','https://unadmitted.invalid','https://fixture.invalid/path']):
            self.frame(url=url+str(index),method='GET',resource_type='Script',headers={'Origin':origin})
        self.assertEqual(self.report()['urls'],[url])
        self.frame(url=url,method='GET',resource_type='Script',headers={})
        self.assertEqual(self.report()['urls'],[])
        self.assertEqual(self.report()['asset_request_origins'],{})
        self.frame(url=url,method='GET',resource_type='Script',headers={'Origin':'https://fixture.invalid'})
        self.assertEqual(self.report()['urls'],[])
        self.assertEqual(len(self.broker.conflicting_proposals),1)

    def test_stylesheet_absence_preserves_disjoint_origins_and_tombstones_unbound_conflicts(self):
        self.broker.host({'id':1,'method':'Page.navigate','sessionId':'page','params':{'url':self.fixture.archive().entry_url}})
        url='https://cdn.fixture.invalid/style.css'
        self.frame(url=url,method='GET',resource_type='Stylesheet',headers={'Accept':'text/css'})
        self.assertEqual(self.report()['asset_origin_absent_stylesheets'],[url])
        self.assertNotIn('asset_origin_absent_stylesheets',self.broker.report())
        for i,headers in enumerate([{}, {'Accept':'text/css','accept':'text/css'}, {'Accept':'text/css\n'}]):
            other=url+str(i)
            self.frame(url=other,method='GET',resource_type='Stylesheet',headers=headers)
            self.assertNotIn(other,self.report()['asset_origin_absent_stylesheets'])
        self.frame(url=url,method='GET',resource_type='Stylesheet',headers={'Origin':'https://fixture.invalid'})
        report=self.report()
        self.assertIn(url,report['urls']);self.assertEqual(report['schema'],2)
        self.assertNotIn(url,report['asset_request_origins']);self.assertNotIn(url,report['asset_origin_absent_stylesheets'])
        self.assertEqual(report['asset_request_variants'][url],[
            {'kinds':['stylesheet'],'request_origin':None,'origin_absent_stylesheet':True},
            {'kinds':['stylesheet'],'request_origin':'https://fixture.invalid','origin_absent_stylesheet':False}])
        self.frame(url=url,method='GET',resource_type='Stylesheet',headers={'Accept':'text/css'})
        self.assertEqual(self.report()['asset_request_variants'][url],report['asset_request_variants'][url])
        other='https://cdn.fixture.invalid/reused.css'
        self.frame(url=other,method='GET',resource_type='Stylesheet',headers={'Accept':'text/css'})
        self.frame(url=other,method='GET',resource_type='Script',headers={'Accept':'*/*'})
        self.assertIn(other,self.report()['urls']);self.assertEqual(self.report()['schema'],3)
        self.assertEqual(len(self.report()['asset_request_variants'][other]),2)
        self.frame(url=other,method='GET',resource_type='Other',headers={})
        self.assertNotIn(other,self.report()['urls'])

    def test_origin_conflict_tombstones_share_the_fixed_proposal_budget(self):
        self.broker.host({'id':1,'method':'Page.navigate','sessionId':'page','params':{'url':self.fixture.archive().entry_url}})
        for index in range(140):
            url='https://cdn.fixture.invalid/'+str(index)
            self.frame(url=url,method='GET',resource_type='Script',headers={'Origin':'https://fixture.invalid'})
            self.frame(url=url,method='GET',resource_type='Script',headers={})
        self.assertEqual(len(self.broker.conflicting_proposals),128)
        self.assertEqual(self.report()['urls'],[])
        self.assertGreater(self.report()['omitted_requests'],0)
        for origin in ['https://fixture.invalid:443','https://FIXTURE.invalid','https://fixture.invalid/path']:
            with self.assertRaises(replay.ReplayError):replay.public_origin(origin)

    def test_published_fetch_preload_matches_observed_other_requests_without_broadening_other_types(self):
        data=b'''<head><base href="https://cdn.fixture.invalid/assets/">
          <link rel="preload" as="fetch" href="bootstrap.data?x=1&amp;y=2">
          <link rel="preload" as="fetch" type="text/html" href="document">
          <template><link rel="preload" as="fetch" href="inert"></template>
          </head><body><link rel="preload" as="fetch" href="body"></body>'''
        (self.fixture.root/'index.html').write_bytes(data)
        self.fixture.files['index.html']=digest(data);self.fixture.resources[0]['body_sha256']=digest(data)
        archive=self.fixture.archive()
        url='https://cdn.fixture.invalid/assets/bootstrap.data?x=1&y=2'
        self.assertEqual(archive.fetch_preloads,{url})
        self.broker=replay.Broker(archive,self.sent.append,self.emitted.append,discovery=True)
        self.broker.sessions['page']='page'
        self.broker.host({'id':1,'method':'Page.navigate','sessionId':'page','params':{'url':archive.entry_url}})
        self.frame(url=url,method='GET',resource_type='Other')
        self.frame(url='https://cdn.fixture.invalid/unpublished',method='GET',resource_type='Other')
        self.assertEqual(self.report()['assets'],{url:['json']})
    def test_context_variants_count_against_the_total_request_budget(self):
        self.broker.host({'id':1,'method':'Page.navigate','sessionId':'page','params':{'url':self.fixture.archive().entry_url}})
        for i in range(64):
            url='https://cdn.fixture.invalid/variant'+str(i)+'.css'
            self.frame(url=url,method='GET',resource_type='Stylesheet',headers={'Origin':'https://fixture.invalid'})
            self.frame(url=url,method='GET',resource_type='Stylesheet',headers={'Accept':'text/css'})
        self.frame(url='https://cdn.fixture.invalid/overflow.css',method='GET',resource_type='Stylesheet',headers={'Accept':'text/css'})
        report=self.report();self.assertEqual(len(report['urls']),64)
        self.assertEqual(sum(len(v) for v in report['asset_request_variants'].values()),128)
        self.assertGreater(report['omitted_requests'],0)

    def test_variant_mismatch_can_propose_only_proven_anonymous_context(self):
        url='https://cdn.fixture.invalid/captured.css'
        self.fixture.add(url,'captured.css',b'input{color:green}',content_type='text/css',headers=[{'name':'Content-Type','value':'text/css'},{'name':'Vary','value':'Origin'}])
        self.fixture.resources[-1]['request_origin']='https://fixture.invalid'
        archive=self.fixture.archive();self.broker=replay.Broker(archive,self.sent.append,self.emitted.append,discovery=True)
        self.broker.sessions['page']='page';self.broker.host({'id':1,'method':'Page.navigate','sessionId':'page','params':{'url':archive.entry_url}})
        self.frame(url=url,method='GET',resource_type='Stylesheet',headers={'Accept':'text/css'})
        self.assertEqual(self.report()['asset_origin_absent_stylesheets'],[url])
        for headers in [{'Accept':'text/css','Cookie':''},{'Accept':'text/css','Authorization':'fictional'},{'Accept':'text/css','accept':'text/css'},{}]:
            self.frame(url=url+'?case='+str(len(headers)),method='GET',resource_type='Stylesheet',headers=headers)
        self.frame(url=url,method='POST',resource_type='Stylesheet',headers={'Accept':'text/css'})
        self.assertGreater(self.report()['ineligible_requests'],0)
        self.assertNotIn('asset_request_variants',self.broker.report())

    def test_unreadable_preload_projection_is_visible_and_does_not_destroy_legacy_replay(self):
        data=b'\xff'
        (self.fixture.root/'index.html').write_bytes(data)
        self.fixture.files['index.html']=digest(data);self.fixture.resources[0]['body_sha256']=digest(data)
        archive=self.fixture.archive()
        self.assertEqual(archive.preload_projection_failures,1)
        self.assertIsNotNone(archive.response({'url':archive.entry_url,'method':'GET'})[0])
        self.broker=replay.Broker(archive,self.sent.append,self.emitted.append,discovery=True)
        self.assertEqual(self.report()['preload_projection_failures'],1)
    def test_frontier_is_bounded_and_overflow_remains_visible(self):
        for index in range(132):self.frame(url=f'https://fixture.invalid/{index}',method='GET')
        report=self.report();self.assertEqual(len(report['urls']),128);self.assertEqual(report['omitted_requests'],4)
        self.frame(url='https://fixture.invalid/'+'x'*4096,method='GET')
        self.assertEqual(self.report()['omitted_requests'],5)
    def test_normal_entry_replay_never_exposes_discovery_urls(self):
        broker=replay.Broker(self.fixture.archive(),self.sent.append,self.emitted.append)
        with self.assertRaisesRegex(replay.ReplayError,'not_resource_discovery'):
            broker.host({'id':1,'method':'SmartJobseeker.discoveryReport','params':{}})
        self.assertNotIn('urls',broker.report());self.assertEqual(self.emitted,[])

if __name__=='__main__':unittest.main()
