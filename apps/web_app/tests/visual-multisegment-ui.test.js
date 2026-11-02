const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const helpers=source.slice(source.indexOf('function visualComparisonStatus('),source.indexOf('function showAutomaticFit('));
const start=source.indexOf(' async function inspectVisual(targetSelector=null,destination=null){'),end=source.indexOf('\n visualButton.addEventListener',start);
assert.ok(start>=0&&end>start);
async function run(secondDom='matches',fresh='same',finalChange=false,finalUnavailable=false){
 const messages=[],storage=new Map(),polls=new Map(),panel={isConnected:true,replaceChildren(){},append(item){if(item?.text)messages.push(item.text);}};
 const context={result:{session_id:'fixture'},fillInProgress:false,fillCompleted:false,visualButton:{disabled:false},fillButton:null,visualPanel:panel,sessionStorage:{getItem:k=>storage.get(k),setItem:(k,v)=>storage.set(k,v),removeItem:k=>storage.delete(k)},element:(_tag,text)=>({text}),document:{createElement:()=>({})},refresh:async()=>{},setTimeout:fn=>fn(),api:async(path,body)=>{
  if(path.endsWith('visual-review-start'))return {task_id:String(body.segment),segment:body.segment,half:body.half,segments:2,page_signature:'fixture',coverage_complete:true,screenshot:'fictional',record_saved:true};
  const count=(polls.get(body.task_id)||0)+1;polls.set(body.task_id,count);if(finalUnavailable && count>1)throw new Error('fixture recheck unavailable');
  const second=body.task_id==='1';return {status:'complete',visual_freshness:finalChange && count>1 && !second?'changed':second?fresh:'same',record_saved:true,visual_review:{observations:second?[{label:'Email',visible_value:'person@example.invalid'}]:[],comparisons:[{label:'Email',expected:'person@example.invalid',status:second?'matches_transcription':'unobserved',...(second?{dom_status:secondDom}:{})}],observation_limit_reached:false}};
 }};
 vm.runInNewContext(helpers,context);
 await vm.runInNewContext(`${source.slice(start,end)}; inspectVisual`,context)();
 return messages.join('\n');
}
(async()=>{
 const reviewed=await run();
 assert.match(reviewed,/Unverified model transcriptions; these can include placeholder text or unrelated page content/);
 assert.match(reviewed,/Email: transcription and live value match reviewed answer/);
 assert.match(await run('unconfirmed'),/Email: live field value not confirmed/);
 assert.match(await run('matches','changed'),/Email: capture changed or freshness could not be confirmed/);
 assert.match(await run('matches','same',true),/Email: capture changed or freshness could not be confirmed/);
 const unavailable=await run('matches','same',false,true);
 assert.match(unavailable,/Visual review incomplete: fixture recheck unavailable/);
 assert.doesNotMatch(unavailable,/transcription and live value match reviewed answer/);
 console.log('Visual multi-segment aggregation checks passed.');
})().catch(e=>{console.error(e);process.exitCode=1});
