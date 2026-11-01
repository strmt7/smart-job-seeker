const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const line=source.split('\n').find(item=>item.startsWith("document.querySelector('#web-search-batch-cancel').addEventListener"));
assert.ok(line);
let click;const calls=[],messages=[];
const ctx={document:{querySelector:()=>({addEventListener:(_type,fn)=>{click=fn}})},webBatchTask:'active-task',sessionStorage:{getItem(){throw Error('unavailable')}},api:async(path,body)=>calls.push({path,body}),notify:message=>messages.push(message)};
vm.runInNewContext(line,ctx);
(async()=>{await click();assert.equal(calls.length,1);assert.equal(calls[0].path,'/api/web-search/batch-cancel');assert.equal(calls[0].body.task_id,'active-task');assert.deepEqual(messages,[]);ctx.webBatchTask=null;await click();assert.equal(calls.length,1);console.log('Batch cancellation without browser storage passed.');})().catch(error=>{console.error(error);process.exitCode=1});
