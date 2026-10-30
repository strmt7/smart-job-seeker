const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf('async function recoverCandidateStart('),end=source.indexOf('\ndocument.querySelector(\'#candidate-batch-start\')',start);
assert.ok(start>=0&&end>start);
const node=()=>({textContent:'',replaceChildren(){},append(){}});
async function run(phase,pending,serverError=false){
 const nodes=new Map(),checks=new Map();let starts=0,statuses=0;
 const context={URL,Number,candidateBatch:{items:[{url:'https://example.test/job',title:'Fictional'}],index:0,phase,task:null,job:phase==='check'?'job-1':null,events:[],stop:false,verify:false},candidateBatchRunning:false,applicationChecks:checks,selectedCandidates:new Map(),renderCandidateBatch(){},saveCandidateBatch(){},updateCandidateSelection(){},refresh:async()=>{},setTimeout:fn=>fn(),element:node,document:{querySelector:key=>{if(!nodes.has(key))nodes.set(key,node());return nodes.get(key);}},api:async(path)=>{if(path.endsWith('/start')){starts++;const e=new Error(serverError?'Source not permitted':'Connection lost');if(serverError)e.httpStatus=400;throw e;}if(path.endsWith('/pending'))return pending;statuses++;return phase==='check'?{status:'complete',checked_url:'https://example.test/job',result:'Review route',current:true}:{status:'complete',saved:{id:'job-1',existing:false},capture:{saved:true,complete:true,structured_source:'captured'}};},applicationCheckState:r=>({url:r.checked_url,text:r.result,current:r.current})};
 const execute=vm.runInNewContext(`${source.slice(start,end)}\nrunCandidateBatch`,context);await execute();return {context,starts,statuses,status:nodes.get('#candidate-batch-status').textContent};
}
(async()=>{
 for(const [phase,pending] of [['listing',{task_id:'task-a',url:'https://example.test/job'}],['check',{task_id:'task-b',job_id:'job-1'}]]){const r=await run(phase,pending);assert.equal(r.starts,1);assert.equal(r.statuses,1);assert.match(r.status,/Finished: 1 of 1/);}
 const other=await run('listing',{task_id:'other',url:'https://example.test/unrelated'});assert.equal(other.statuses,0);assert.match(other.status,/Inspection paused: Another browser inspection/);assert.equal(other.context.candidateBatch.index,0);
 const unknown=await run('listing',{task_id:null});assert.equal(unknown.statuses,0);assert.match(unknown.status,/Inspection paused: Connection lost/);assert.equal(unknown.context.candidateBatch.index,0);
 const rejected=await run('listing',{task_id:null},true);assert.match(rejected.status,/Finished: 1 of 1/);assert.equal(rejected.statuses,0);
 console.log('Candidate start-response recovery checks passed.');
})().catch(e=>{console.error(e);process.exitCode=1;});
