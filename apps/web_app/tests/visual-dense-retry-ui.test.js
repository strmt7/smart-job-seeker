const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf(' async function inspectVisual(){');
const end = source.indexOf('\n visualButton.addEventListener', start);
assert.ok(start >= 0 && end > start);
const calls = [], store = new Map();
const panel = {isConnected:true, replaceChildren(){}, append(){}};
const context = {
  result:{session_id:'fictional'}, fillInProgress:false, fillCompleted:false,
  visualButton:{disabled:false}, fillButton:null, visualPanel:panel,
  sessionStorage:{getItem:k=>store.get(k),setItem:(k,v)=>store.set(k,v),removeItem:k=>store.delete(k)},
  element:()=>({}), document:{createElement:()=>({})}, refresh:async()=>{},
  setTimeout:fn=>fn(), visualComparisonStatus:()=>'',
  api:async(path, body)=>{
    if(path === '/api/form/visual-review-start'){
      calls.push(body);
      return {task_id:String(calls.length), segment:body.segment, half:body.half, segments:1, page_signature:'page', coverage_complete:true, screenshot:'fixture', record_saved:true};
    }
    const full = body.task_id === '1';
    return {status:'complete', visual_freshness:'same', record_saved:true,
      visual_review:{observations:full ? Array(20).fill({label:'x',visible_value:'y'}) : [], comparisons:[], observation_limit_reached:full}};
  }
};
(async()=>{
  const run = vm.runInNewContext(`${source.slice(start,end)}; inspectVisual`,context);
  await run();
  assert.deepEqual(calls.map(x=>x.half),[null,0,1]);
  assert.equal(store.size,0);
  assert.equal(context.visualButton.disabled,false);
  console.log('Dense visual segment retry UI check passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});


