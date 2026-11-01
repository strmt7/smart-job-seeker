const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),test=require('node:test');
const src=fs.readFileSync('apps/web_app/web/app.js','utf8'),helper=src.split('\n').find(line=>line.startsWith('function savedListingReinspection('));
function node(tag,text){return {tag,text,disabled:false,addEventListener(name,fn){this[name]=fn}}}
test('saved job reinspection opens only the matching saved record',async()=>{
 let result={saved:{id:'job-1'}}, shown=0, rendered=0, calls=[];
 const ctx={element:node,api:async(path,body)=>{calls.push([path,body]);return {task_id:'task-1'}},sessionStorage:{setItem(){}},notify(){},followListingInspection:async()=>result,renderJobs(){rendered++},showReinspectionPreview(){shown++},document:{querySelector:()=>({})},offerListingRecovery:async()=>{},activeJobId:null};
 vm.runInNewContext(helper,ctx);const button=ctx.savedListingReinspection({id:'job-1',canonical_url:'https://example.org/job'});await button.click();assert.equal(calls[0][0],'/api/listing-inspect/start');assert.equal(calls[0][1].url,'https://example.org/job');assert.equal(shown,1);assert.equal(rendered,1);assert.equal(ctx.activeJobId,'job-1');
 result={saved:{id:'other'}};await button.click();assert.equal(shown,1);assert.equal(button.disabled,false);
});
