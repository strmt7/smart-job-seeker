const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf('async function captureFilledFieldViews(');
const end = source.indexOf('\nfunction showAutomaticFit(', start);
assert.ok(start >= 0 && end > start);
const capture = vm.runInNewContext(`${source.slice(start, end)}; captureFilledFieldViews`);
const ok = () => ({record_saved:true,failed_attachments:0,unverified_attachments:0,value_retained:true,visual_freshness:'same',unverified_count:0,failed_fields:[]});

(async () => {
  let calls = [];
  let result = await capture(async (path, body) => { calls.push([path, body.selector]); return ok(); }, 'session', Array.from({length:15}, (_,i)=>`#f${i}`), ()=>{}, ()=>true);
  assert.equal(calls.length, 12);
  assert.equal(result.saved, 12);
  assert.equal(result.total, 15);
  assert.match(result.reason, /limit/);
  calls = [];
  result = await capture(async (_path, body) => { calls.push(body.selector); return calls.length === 2 ? {...ok(), unverified_count:1} : ok(); }, 'session', ['#a','#b','#c'], ()=>{}, ()=>true);
  assert.deepEqual(calls, ['#a','#b']);
  assert.equal(result.saved, 2);
  assert.match(result.reason, /could not be verified/);
  calls = [];
  result = await capture(async (_path,body) => {calls.push(body.selector);return {...ok(),failed_attachments:1};}, 'session', ['#a','#b'], ()=>{}, ()=>true);
  assert.deepEqual(calls,['#a']);
  assert.equal(result.saved,1);
  assert.match(result.reason,/attachment/);
  result = await capture(async () => ({...ok(), record_saved:false}), 'session', ['#a'], ()=>{}, ()=>true);
  assert.equal(result.saved, 0);
  assert.match(result.reason, /could not be saved/);
  result = await capture(async () => { throw Error('session expired'); }, 'session', ['#a'], ()=>{}, ()=>true);
  assert.match(result.reason, /session expired/);
  let activeCalls = 0;
  result = await capture(async () => { activeCalls++; return ok(); }, 'session', ['#a'], ()=>{}, ()=>false);
  assert.equal(activeCalls, 0);
  assert.match(result.reason, /closed/);
  const handlerStart = source.indexOf("captureFieldsButton.addEventListener('click',");
  const handlerEnd = source.indexOf('\n async function focusFieldVision(', handlerStart);
  assert.ok(handlerStart >= 0 && handlerEnd > handlerStart);
  let handler;
  const button = () => ({disabled:false});
  const children = [];
  const ctx = {
    captureFieldsButton: {disabled:false, addEventListener:(_event, fn)=>{handler=fn;}},
    fieldFocusButton:button(), fieldVisionButton:button(), visualButton:button(),
    fieldFocusSelect:{options:[{value:'#a'},{value:'#b'}],selectedIndex:0}, fillInProgress:false,
    panel:{isConnected:true}, result:{session_id:'fixture'}, captureFilledFieldViews:capture,
    visualPanel:{replaceChildren:(...items)=>{children.splice(0,children.length,...items);}},
    element:(_tag,text)=>({text}), refresh:async()=>{},
    api:async (_path,body)=> body.selector==='#b' ? {...ok(),value_retained:false} : ok(),
  };
  vm.runInNewContext(source.slice(handlerStart,handlerEnd),ctx);
  await handler();
  assert.ok(children[0].text.includes('2 of 2 remaining focused views saved'));
  assert.ok(children[0].text.includes('could not be verified'));
  assert.ok(children[1].text.includes('do not prove'));
  assert.equal(ctx.captureFieldsButton.disabled,false);
  assert.equal(ctx.visualButton.disabled,false);
  assert.equal(ctx.fieldFocusSelect.selectedIndex,0);
  ctx.fieldFocusSelect.options=Array.from({length:15},(_,i)=>({value:`#f${i}`}));
  const visited=[];
  ctx.api=async (_path,body)=>{visited.push(body.selector);return ok();};
  await handler();
  assert.equal(ctx.fieldFocusSelect.selectedIndex,12);
  assert.equal(visited.length,12);
  await handler();
  assert.deepEqual(visited.slice(12),['#f12','#f13','#f14']);
  assert.ok(children[0].text.includes('3 of 3 remaining focused views saved'));
})().catch(error => { console.error(error); process.exitCode = 1; });
