const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const line=source.split('\n').find(x=>x.startsWith('async function recheckExportText('));
assert.ok(line);
(async()=>{
  const calls=[];
  const context={setTimeout:fn=>fn(),api:async(path,body)=>{calls.push([path,body]);return path.endsWith('-start')?{task_id:'fixture'}:{status:'complete',text_check:{status:'matches'},record_saved:true};}};
  const run=vm.runInNewContext(`(${line})`,context);
  const output={textContent:''};
  await run({job_id:'j',id:'d'},output);
  assert.match(output.textContent,/matched.*result was saved.*visual layout/);
  assert.equal(calls[0][1].document_id,'d');
  context.api=async()=>{throw new Error('missing export record')};
  await run({job_id:'j',id:'d'},output);
  assert.match(output.textContent,/missing export record/);
  console.log('Export text recheck UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1});
