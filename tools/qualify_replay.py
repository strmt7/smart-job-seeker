"""Explicit fictional response replay under AppContainer and a bounded Windows job."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
from qualify_rehearsal import inventory,fictional_pdf,run,test_denominator
from qualify_history_ui import RamSamples

REPO=Path(__file__).resolve().parent.parent


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine',required=True,type=Path)
    parser.add_argument('--python',type=Path,default=Path(sys.executable))
    parser.add_argument('--frame-churn',action='store_true',help='Replace 80 cross-site recorded frames before continuing the original fixture')
    parser.add_argument('--alias-lifecycle',action='store_true',help='Also verify physical alias closure and fresh attachment on a live target')
    parser.add_argument('--frame-controls',action='store_true',help='Qualify a recorded child target with actual typed controls')
    args=parser.parse_args()
    if args.frame_controls and (args.frame_churn or args.alias_lifecycle):parser.error('Frame controls run separately from lifecycle/churn qualification')
    if os.name!='nt' or not args.engine.is_absolute() or not args.python.is_absolute():
        parser.error('Windows and explicit absolute runtime paths required')
    files=inventory(args.engine);args.engine=args.engine.resolve(strict=True)
    if not (args.engine/'chrome-headless-shell.exe').is_file():parser.error('Headless-shell engine required')
    folder=Path(tempfile.mkdtemp(prefix='replay-qualification-',dir=REPO/'target'));content=folder/'content';content.mkdir()
    resources=[]
    def add(url,name,body,mime,method='GET',request=b''):
        data=body.encode() if isinstance(body,str) else body
        if (args.frame_churn or args.frame_controls) and name=='index.html':
            data += b'<label>Frame cycles<input id="churn-status" value="0"></label><button id="start-churn" type="button">Start frame cycles</button><label>Interactive frame status<input id="interactive-status" value="idle"></label><button id="open-inspected" type="button">Open interactive frame</button><button id="close-inspected" type="button">Close interactive frame</button><script>(()=>{const status=document.getElementById(\'churn-status\'),button=document.getElementById(\'start-churn\');let frame=null,count=0;const spawn=()=>{frame=document.createElement(\'iframe\');frame.src=\'https://steps.example.invalid/frame\';document.body.append(frame);};window.addEventListener(\'message\',event=>{if(event.origin!==\'https://steps.example.invalid\'||event.source!==frame?.contentWindow||event.data!==\'frame-cycle-ready\')return;frame.remove();frame=null;status.value=String(++count);if(count<80)setTimeout(spawn,15);});button.addEventListener(\'click\',()=>{button.disabled=true;spawn();});let inspected=null;document.getElementById(\'open-inspected\').addEventListener(\'click\',()=>{inspected=document.createElement(\'iframe\');inspected.src=\'https://steps.example.invalid/interactive\';document.body.append(inspected);});window.addEventListener(\'message\',event=>{if(event.origin===\'https://steps.example.invalid\'&&event.source===inspected?.contentWindow&&event.data===\'interactive-frame-ready\')document.getElementById(\'interactive-status\').value=\'ready\';});document.getElementById(\'close-inspected\').addEventListener(\'click\',()=>{inspected?.remove();inspected=null;document.getElementById(\'interactive-status\').value=\'removed\';});})();</script>'
        (content/name).write_bytes(data)
        resources.append({'url':url,'method':method,'request_body_sha256':hashlib.sha256(request).hexdigest(),
                          'status':200,'headers':[{'name':'Content-Type','value':mime}],
                          'body_file':name,'body_sha256':hashlib.sha256(data).hexdigest()})
    add('https://fixture.invalid/form','index.html','''<!doctype html><title>Fictional replay</title><link rel="icon" href="data:,"><link rel="stylesheet" href="/app.css"><h1>Fictional recorded application</h1><p>Offline qualification. No application delivery.</p><label>Name<input id="name"></label><label>Bootstrap status<input id="bootstrap-status"></label><label>Frame status<input id="frame-status"></label><label>Worker status<input id="worker-status"></label><label>Blocked request status<input id="blocked-status"></label><button id="next" type="button">Continue</button><label>Step status<input id="step-status"></label><section id="questions"></section><label>Fictional PDF<input id="resume" type="file" accept=".pdf"></label><label>Script-triggered request probe<input id="sideeffect"></label><button id="submit" type="submit">Send application</button><iframe src="https://child.fixture.invalid/frame"></iframe><script src="/app.js"></script>''','text/html')
    add('https://fixture.invalid/app.css','style.bin','body{font:16px Arial;margin:24px}label{display:block;margin:10px}input{margin-left:12px}iframe{width:300px;height:100px}','text/css')
    add('https://fixture.invalid/app.js','script.bin','''const el=id=>document.getElementById(id);fetch('/bootstrap',{method:'POST',headers:{'Content-Type':'application/json'},body:'{"operation":"bootstrap"}'}).then(r=>r.json()).then(v=>el('bootstrap-status').value=v.status);window.addEventListener('message',event=>{if(event.origin==='https://child.fixture.invalid'&&event.data==='frame-ready')el('frame-status').value='ready';});const worker=new Worker('/worker.js');worker.onmessage=e=>el('worker-status').value=e.data;el('sideeffect').addEventListener('input',()=>fetch('/uncaptured-application?candidate='+encodeURIComponent(el('sideeffect').value),{method:'POST',body:el('sideeffect').value}).catch(()=>el('blocked-status').value='blocked'));el('next').addEventListener('click',()=>fetch('/questions').then(r=>r.json()).then(v=>{const label=document.createElement('label');label.textContent=v.question;const input=document.createElement('input');input.id='email';input.type='email';label.append(input);el('questions').replaceChildren(label);el('step-status').value='questions loaded';}));''','text/javascript')
    add('https://fixture.invalid/bootstrap','bootstrap.bin','{"status":"ready"}','application/json','POST',b'{"operation":"bootstrap"}')
    add('https://fixture.invalid/questions','questions.bin','{"question":"Email"}','application/json')
    add('https://child.fixture.invalid/frame','frame.bin','<!doctype html><p>Fictional recorded frame</p><script>parent.postMessage("frame-ready","https://fixture.invalid");</script>','text/html')
    add('https://fixture.invalid/worker.js','worker.bin',"fetch('/worker-data').then(r=>r.json()).then(v=>postMessage(v.value)).catch(()=>postMessage('unavailable'));",'text/javascript')
    add('https://fixture.invalid/worker-data','worker-data.bin','{"value":"recorded-worker"}','application/json')
    if args.frame_churn:
        add('https://steps.example.invalid/frame','frame-cycle.bin','<!doctype html><title>Fictional cycle frame</title><script>parent.postMessage("frame-cycle-ready","https://fixture.invalid")</script>','text/html')
    if args.frame_churn or args.frame_controls:
        add('https://steps.example.invalid/interactive','interactive-frame.bin','<!doctype html><label>Cycle answer<input id="cycle-answer"></label><label>Remote work<input type="checkbox" id="remote"></label><label>Team<select id="team"><option>Choose</option><option>Platform</option></select></label><label>Motivation<textarea id="motivation"></textarea></label><label>Resume<input type="file" id="child-resume" accept=".pdf"></label><button type="button" id="child-next" onclick="document.getElementById(\'child-later\').hidden=false">Continue section</button><section id="child-later" hidden><label>Followup<input id="followup"></label></section><script>parent.postMessage("interactive-frame-ready","https://fixture.invalid")</script>','text/html')
    (content/'fictional.pdf').write_bytes(fictional_pdf())
    manifest={'schema':1,'body_representation':'decoded_http','entry_url':'https://fixture.invalid/form#application','resources':resources}
    (content/'replay.json').write_text(json.dumps(manifest),encoding='utf-8')
    config={'schema':2,'engine_root':str(args.engine),'engine_files':files,'content_root':str(content),
            'content_files':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in content.iterdir()},
            'entry':'index.html','replay_manifest':'replay.json','seconds':180 if args.frame_controls else 120}
    path=folder/'configuration.private.json';path.write_text(json.dumps(config),encoding='utf-8')
    sources=[REPO/p for p in ['crates/waypoint_browser/src/lib.rs','crates/waypoint_browser/src/captured_frame.rs','crates/waypoint_browser/src/rehearsal.rs','crates/waypoint_browser/src/rehearsal_worker.py','crates/waypoint_browser/src/rehearsal_replay.py','crates/waypoint_browser/tests/rehearsal_replay_live.rs','tools/owned_process.py','tools/qualify_rehearsal.py','tools/qualify_history_ui.py','tools/qualify_replay.py','crates/waypoint_browser/src/replay_lifecycle_qualification.rs']]
    def hashes():return {str(p.relative_to(REPO)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}
    report={'complete':False,'frame_churn':args.frame_churn,'alias_lifecycle':args.alias_lifecycle,'frame_controls':args.frame_controls,'sources':hashes(),'config_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'scope':'fictional HTTPS-origin recorded responses only; no general network isolation claim'}
    environment=os.environ.copy();environment.update(WAYPOINT_REHEARSAL_CONFIG=str(path),WAYPOINT_REHEARSAL_CONFIG_SHA256=report['config_sha256'],WAYPOINT_REHEARSAL_PYTHON=str(args.python),WAYPOINT_REPLAY_FRAME_CHURN='1' if args.frame_churn else '0',CARGO_BUILD_JOBS='2')
    memory=RamSamples();memory.thread.start()
    try:
        log=folder/'native.private.log'
        selection=['--lib','replay_lifecycle_qualification::recorded_child_scope_preserves_origin_controls_and_parent'] if args.frame_controls else ['--test','rehearsal_replay_live']
        report['run']=run([shutil.which('cargo'),'test','-p','waypoint_browser',*selection,'--','--ignored','--nocapture','--test-threads=1'],environment,log,240)
        if report['run']['exit_code']:raise RuntimeError('Replay failed; inspect '+str(log))
        report['tests']=test_denominator(log.read_text(encoding='utf-8'),'rust',1)
        native=json.loads((folder/'native.private.json').read_text());assert native['complete'] is True
        report['native_sha256']=hashlib.sha256((folder/'native.private.json').read_bytes()).hexdigest()
        (folder/'screenshot.png').write_bytes(base64.b64decode((folder/'replay-screenshot.base64').read_text(),validate=True))
        if inventory(args.engine)!=files:raise RuntimeError('Source engine changed during qualification')
        report['source_engine_unchanged']=True
        if hashes()!=report['sources']:raise RuntimeError('Replay source changed during qualification')
        if args.alias_lifecycle:
            alias_log=folder/'alias-lifecycle.private.log'
            report['alias_run']=run([shutil.which('cargo'),'test','-p','waypoint_browser','--lib','replay_lifecycle_qualification::closed_interceptor_releases_aliases_before_reattachment','--','--ignored','--nocapture','--test-threads=1'],environment,alias_log,180)
            if report['alias_run']['exit_code']:raise RuntimeError('Alias lifecycle failed; inspect '+str(alias_log))
            report['alias_tests']=test_denominator(alias_log.read_text(encoding='utf-8'),'rust',1)
            alias=folder/'alias-lifecycle.private.json'
            if json.loads(alias.read_text())['complete'] is not True:raise RuntimeError('Missing alias lifecycle completion')
            report['alias_native_sha256']=hashlib.sha256(alias.read_bytes()).hexdigest()
            if hashes()!=report['sources']:raise RuntimeError('Replay source changed during alias qualification')
        report['complete']=True
    finally:
        report['memory']=memory.finish();(folder/'result.private.json').write_text(json.dumps(report,indent=2),encoding='utf-8');print('Private replay qualification:',folder)


if __name__=='__main__':main()
