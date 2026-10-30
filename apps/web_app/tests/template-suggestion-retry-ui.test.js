const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf('async function followTemplateSuggestion('),end=source.indexOf('\nfunction batchTemplateSuggestions(',start);
assert.ok(start>=0 && end>start);
async function run(responses){
 const calls=[];
 const context={setTimeout:callback=>callback(),api:async(path,body)=>{calls.push({path,body});const next=responses.shift();if(next instanceof Error)throw next;return next;}};
 const follow=vm.runInNewContext(`${source.slice(start,end)}; followTemplateSuggestion`,context);
 try{return {result:await follow('fictional-task'),calls};}catch(error){return {error,calls};}
}
(async()=>{
 const recovered=await run([new Error('lost response'),new Error('temporary failure'),{status:'running'},{status:'complete',suggestion:{text:'fixture'}}]);
 assert.equal(recovered.result.status,'complete');assert.equal(recovered.calls.length,4);
 assert.ok(recovered.calls.every(call=>call.path==='/api/documents/template-suggest-status' && call.body.task_id==='fictional-task'));
 const failed=await run([new Error('one'),new Error('two'),new Error('three')]);
 assert.equal(failed.calls.length,3);assert.equal(failed.error.message,'three');
 console.log('Template suggestion retry checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
