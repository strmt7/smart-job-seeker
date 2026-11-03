const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf('async function recoverWebBatch('),end=source.indexOf('\nvoid recoverWebBatch()',start);
assert.ok(start>=0 && end>start);
const storage=new Map([['waypoint-web-search-batch',JSON.stringify({task:'stale',groups:[]})]]),status={textContent:''},messages=[];
let rendered=null;
const context={sessionStorage:{getItem:key=>storage.get(key)||null,setItem:(key,value)=>storage.set(key,value),removeItem:key=>storage.delete(key)},
  followWebBatch:async()=>{throw Error('Batch web search not found');},
  api:async path=>{assert.equal(path,'/api/web-search/batch-pending');return {active:false,last:{task_id:'saved',result:{status:'complete',history_saved:true,prior_history:[{query:'earlier',page:1,data:{results:[]}}],history:[{query:'fictional',page:1,data:{results:[{url:'https://example.test/job'}]}}]}}};},
  renderWebBatch:value=>{rendered=value;},webBatchSummary:(_result,prefix)=>prefix+'recovered',
  document:{querySelector:()=>status},notify:message=>messages.push(message)};
(async()=>{const recover=vm.runInNewContext(`${source.slice(start,end)}; recoverWebBatch`,context);await recover();assert.equal(storage.has('waypoint-web-search-batch'),false);assert.equal(rendered.groups[0].data.results[0].url,'https://example.test/job');assert.equal(rendered.priorGroups[0].query,'earlier');assert.equal(rendered.task,'saved');assert.equal(status.textContent,'Previous recovered');assert.equal(messages.length,0);
rendered=null;const denied={...context,webBatchStorageWarning:false,sessionStorage:{getItem(){throw Error('disabled')},removeItem(){throw Error('disabled')}},notify:message=>messages.push(message)};const recoverDenied=vm.runInNewContext(`${source.slice(start,end)}; recoverWebBatch`,denied);await recoverDenied();assert.equal(rendered.groups[0].data.results[0].url,'https://example.test/job');assert.match(messages[0],/checking the backend/);let resumed;const active={...context,sessionStorage:{getItem:()=>JSON.stringify({task:'active',groups:[]})},api:async()=>({active:true,task_id:'active',prior_history:[{query:'earlier',page:1,data:{results:[]}}]}),followWebBatch:async batch=>{resumed=batch;}};await vm.runInNewContext(`${source.slice(start,end)}; recoverWebBatch`,active)();assert.equal(resumed.priorGroups[0].query,'earlier');console.log('Web-search restart recovery UI checks passed.');})().catch(error=>{console.error(error);process.exitCode=1;});
