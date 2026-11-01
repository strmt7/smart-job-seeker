const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),test=require('node:test');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8'),line=source.split('\n').find(s=>s.startsWith('function automaticFitFailureText('));const ctx={};vm.runInNewContext(line,ctx);
test('automatic fit failure names the job and preserves a bounded reason',()=>{
 const jobs=[{job:{id:'1',title:'Fictional engineer'}}];
 assert.equal(ctx.automaticFitFailureText('1',{error:'Invalid criterion quote'},jobs),'Fictional engineer: Invalid criterion quote');
 assert.equal(ctx.automaticFitFailureText('missing',{},jobs),'Listing: Assessment could not be saved');
 assert.ok(ctx.automaticFitFailureText('1',{error:'x'.repeat(1000)},jobs).length<280);
});
