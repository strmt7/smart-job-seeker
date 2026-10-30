const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js','utf8');
const start = source.indexOf('let letterExportRecovering=false;');
const end = source.indexOf('\nvoid recoverPendingLetterExport(',start);
assert.ok(start >= 0 && end > start);
function checkDraftFraming(){
 const line=source.split('\n').find(item=>item.startsWith('function showLetter('));
 const evidence=source.split('\n').find(item=>item.startsWith('function showDocumentEvidence('));
 const selection=source.split('\n').find(item=>item.startsWith('function draftSelectionSummary('));
 const selected=source.split('\n').find(item=>item.startsWith('function selectedFactsDetails('));
 assert.ok(line);
 assert.ok(evidence && selection && selected);
 const children=[], panel={replaceChildren(...items){children.push(...items);},append(...items){children.push(...items);}};
 const make=(tag,text='')=>({tag,text,value:'',setAttribute(){},addEventListener(){}});
 const show=vm.runInNewContext(`${evidence}\n${selection}\n${selected}\n${line}; showLetter`,{setWorkspaceView(){},document:{querySelector:()=>panel,createElement:make},element:make});
 const body={paragraphs:[{text:'I used Python.',facts:[1]}],facts:['Used Python'],notice:'Review',kind:'cover_letter',template_used:false,framing:{opening:'I am applying for the Scientist position at Example Lab.',closing:'Thank you for considering my application.'}};
 show({id:'fixture',title:'Scientist',employer:'Example Lab'},body,'fixture');
 assert.equal(children.find(item=>item.tag==='textarea').value,'I am applying for the Scientist position at Example Lab.\n\nI used Python.\n\nThank you for considering my application.');
 assert.equal(children.filter(item=>String(item.text).startsWith('Cited qualifications:')).length,1);
}
async function fixture(active, saved){
 const storage=new Map(saved ? [['waypoint-letter-export',JSON.stringify(saved)]] : []), calls=[], shown=[];
 const panel={replaceChildren(){},append(){}};
 const context={sessionStorage:{getItem:key=>storage.get(key)||null,setItem:(key,value)=>storage.set(key,value),removeItem:key=>storage.delete(key)},element:()=>({}),
  api:async(path)=>{calls.push(path);return active;},followLetterExport:async(task)=>{calls.push(task);storage.delete('waypoint-letter-export');return {id:'fictional',name:'fictional.pdf'};},
  showExportedPdf:(_,result)=>shown.push(result),refresh:async()=>{},notify:()=>{}};
 const recover=vm.runInNewContext(`${source.slice(start,end)}; recoverPendingLetterExport`,context);
 return {recovered:await recover(panel),calls,shown,storage};
}
(async()=>{
 checkDraftFraming();
 const running=await fixture({task_id:'new-task',job_id:'fictional-job'}, {task:'old-task',job:'old-job'});
 assert.equal(running.recovered,true);assert.deepEqual(running.calls,['/api/letter/export-pending','new-task']);assert.equal(running.shown.length,1);assert.equal(running.storage.size,0);
 const replay=await fixture({task_id:null},{task:'saved-task',job:'fictional-job'});
 assert.equal(replay.recovered,true);assert.equal(replay.calls[1],'saved-task');
 const empty=await fixture({task_id:null},null);assert.equal(empty.recovered,false);assert.equal(empty.shown.length,0);
 console.log('PDF export recovery UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
