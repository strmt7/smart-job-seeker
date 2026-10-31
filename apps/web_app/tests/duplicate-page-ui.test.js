const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf(' async function runDuplicateSearch(paged){'),end=source.indexOf("\n findDuplicates.addEventListener",start);
assert.ok(start>=0 && end>start);
async function run(initial,result,paged=true){
 const data=new Map(initial),requests=[],messages=[];
 const context={job:{id:'fictional'},findDuplicates:{},scanMore:{},setTimeout:fn=>fn(),refresh:async()=>{},notify:value=>messages.push(value),
  sessionStorage:{getItem:k=>data.get(k)||null,setItem:(k,v)=>data.set(k,v),removeItem:k=>data.delete(k)},
  api:async(path,body)=>{requests.push({path,body});if(path.endsWith('search-pending'))return {active:false};if(path.endsWith('search-start'))return {task_id:'task'};return result;}};
 await vm.runInNewContext(`${source.slice(start,end)};runDuplicateSearch`,context)(paged);
 return {data,requests,messages};
}
(async()=>{
 const result={status:'complete',page_after:'',next_cursor:'job-12',examined:12,unexamined:0,total_candidates:12,added:0,truncated:true};
 let x=await run([],result);assert.equal(x.requests[1].body.after,'');assert.deepEqual(JSON.parse(x.data.get('waypoint-duplicate-cursor')),{job:'fictional',after:'job-12'});
 x=await run([...x.data],{...result,page_after:'job-12',next_cursor:'job-24',failed_batches:1,unexamined:1,examined:11});assert.equal(x.requests[1].body.after,'job-12');assert.equal(JSON.parse(x.data.get('waypoint-duplicate-cursor')).after,'job-12');
 let recovered=await run([...x.data],{...result,page_after:'job-12',next_cursor:'job-24',failed_batches:1,retried_candidates:3});assert.equal(JSON.parse(recovered.data.get('waypoint-duplicate-cursor')).after,'job-24');
 x=await run([...x.data],{...result,page_after:'job-12',next_cursor:null,truncated:false});assert.equal(x.data.has('waypoint-duplicate-cursor'),false);
 x=await run([],{...result,page_after:null,next_cursor:null},false);assert.equal(JSON.parse(x.data.get('waypoint-duplicate-cursor')).after,'');
 console.log('Duplicate page UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
