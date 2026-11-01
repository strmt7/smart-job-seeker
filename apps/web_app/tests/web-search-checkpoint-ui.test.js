const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf('async function followWebBatch('),end=source.indexOf("\ndocument.querySelector('#web-search-batch')",start);
assert.ok(start>=0 && end>start);
const seen=[],status={set textContent(value){seen.push(value);}},stop={hidden:true},storage=new Map();let calls=0;
const context={webBatchRunning:false,document:{querySelector:id=>id==='#web-search-batch-status'?status:stop},renderWebBatch:()=>{},webBatchSummary:()=> 'finished',setTimeout:fn=>fn(),sessionStorage:{setItem:(key,value)=>storage.set(key,value),removeItem:key=>storage.delete(key)},api:async()=>++calls===1?{status:'running',processed:1,total:2,checkpoint_failed:true,query:'fictional',page:1,data:{results:[]}}:{status:'complete',processed:1,total:2,history:[]}};
(async()=>{const follow=vm.runInNewContext(`${source.slice(start,end)}; followWebBatch`,context);await follow({task:'fixture',groups:[]});assert.ok(seen.some(value=>value.includes('progress could not be saved')));assert.equal(seen.at(-1),'finished');console.log('Web-search checkpoint UI checks passed.');})().catch(error=>{console.error(error);process.exitCode=1;});
