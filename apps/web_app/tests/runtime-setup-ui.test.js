'use strict';
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),path=require('node:path');
const context={};vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../web/runtime-setup.js'),'utf8'),context);
function fixture(api,component='missing'){
  const button={disabled:false},info={textContent:''},values=new Map(),storage={getItem:key=>values.get(key)||null,setItem:(key,value)=>values.set(key,value),removeItem:key=>values.delete(key)};
  const form={querySelector:()=>button,elements:{component:{value:component}},addEventListener:()=>{}};
  return {...context.SmartJobseekerToolSetup.connect({form,info,api,storage,wait:async()=>{}}),info,button,values};
}
(async()=>{
 let calls=[],statuses=0;
 const ready=fixture(async(url,body)=>{calls.push({url,body});if(url.endsWith('/start'))return{task_id:'one'};if(statuses++===0)return{status:'running',phase:'Installing browser'};return{status:'complete',result:{components:[{tool:'pdf'},{tool:'browser'},{tool:'office',already_available:true}]}};});
 await ready.start();assert.equal(ready.info.textContent,'Tools ready: PDF tools, browser, document converter.');assert.equal(ready.button.disabled,false);assert.equal(ready.values.size,0);assert.equal(calls.filter(c=>c.url.endsWith('/start')).length,1);
 let starts=0;
 const lost=fixture(async url=>{if(url.endsWith('/start')){starts++;throw Error('Response lost');}if(url.endsWith('/pending'))return{task_id:'recovered'};return{status:'complete',result:{components:[{tool:'ollama'}]}};},'ollama');
 await lost.start();assert.equal(starts,1);assert.equal(lost.info.textContent,'Tools ready: Ollama.');assert.equal(lost.values.size,0);
 let fail=true;
 const retry=fixture(async url=>{if(url.endsWith('/start'))return{task_id:'retained'};if(fail)throw Error('Temporary status failure');return{status:'complete',result:{components:[]}};});
 await retry.start();assert.equal(retry.values.get('smart-jobseeker-tool-setup'),'retained');fail=false;await retry.recover();assert.equal(retry.values.size,0);
 const refused=fixture(async url=>url.endsWith('/start')?{task_id:'refused'}:{status:'failed',error:'<script>untrusted error</script>'});
 await refused.start();assert.equal(refused.info.textContent,'<script>untrusted error</script>');assert.equal(refused.values.size,0);assert.equal(refused.button.disabled,false);
 const cleanup=fixture(async url=>url.endsWith('/start')?{task_id:'warning'}:{status:'complete',result:{components:[{runtime:{tool:'browser'},cleanup_warning:'Private setup files remain'}]}});
 await cleanup.start();assert.equal(cleanup.info.textContent,'Tools ready: browser. Private setup files remain');
 let polls=0;
 const bounded=fixture(async url=>url.endsWith('/start')?{task_id:'running'}:(polls++,{status:'running'}));
 await bounded.start();assert.equal(polls,3000);assert.equal(bounded.values.get('smart-jobseeker-tool-setup'),'running');assert.match(bounded.info.textContent,/may still be running/);
 const html=fs.readFileSync(path.join(__dirname,'../web/index.html'),'utf8');assert.ok(html.includes('id="tools-setup-form"'));assert.ok(html.includes('src="/runtime-setup.js"'));assert.equal((html.match(/id="tools-setup-info"/g)||[]).length,1);
 console.log('Runtime setup selection, recovery, failures, bounds and safe text checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
