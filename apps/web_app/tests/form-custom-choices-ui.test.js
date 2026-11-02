const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf(' const choicesButton=');
const end = source.indexOf('\n const sessionEstimate=', start);
assert.ok(start >= 0 && end > start);
class Node {
  constructor(tag, text='') { this.tag=tag; this.text=text; this.children=[]; this.listeners={}; }
  setAttribute() {}
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children=children; }
  addEventListener(name, fn) { this.listeners[name]=fn; }
  click() { return this.listeners.click?.(); }
}
const panel = new Node('panel');
const element = (tag, text='') => new Node(tag, text);
const inventory = {choices:[{label:'Job location',selector:'#location',expanded:true,options:['Berlin','Lisbon'],options_incomplete:true},{label:'Department',selector:'#department',expanded:false,options:[]}],incomplete:false,snapshot_digest:'a'.repeat(64)};
const calls=[];
const api = async (path, body) => {
  calls.push(path);
  if(path==='/api/form/custom-choices'){assert.equal(body.session_id,'session');return inventory;}
  if(path==='/api/form/control-vision-pending')return {active:false};
  if(path==='/api/form/custom-choice-advice-pending')return {active:false};
  if(path==='/api/form/custom-choice-advice-start'){if(body.answer==='Berlin')return {status:'complete',session_id:'session',radio:false,method:'exact',advice:{option:'Berlin',evidence:'Berlin'}};assert.equal(body.answer,'I prefer Berlin');return {task_id:'advice'};}
  if(path==='/api/form/custom-choice-advice-status')return {status:'complete',session_id:'session',radio:false,advice:{option:'Berlin',evidence:'Berlin'}};
  if(path==='/api/form/custom-choice-option-vision-start'){assert.equal(body.selector,'#location');assert.equal(body.option,'Berlin');return {task_id:'option-vision'};}
  if(path==='/api/form/custom-choice-vision-start'){assert.equal(body.selector,'#location');assert.equal(body.snapshot_digest,inventory.snapshot_digest);return {task_id:'vision'};}
  if(path==='/api/form/control-vision-status')return body.task_id==='option-vision'?{status:'complete',rank_task_id:'custom-choice-option',session_id:'session',label:'Berlin',comparison:'label_observed',capture_saved:true,result_saved:true}:{status:'complete',choice:true,session_id:'session',label:'Job location',comparison:'label_observed',capture_saved:true,result_saved:true};
  throw new Error(`Unexpected request ${path}`);
};
const stored = new Map();
const sessionStorage={getItem:key=>stored.get(key),setItem:(key,value)=>stored.set(key,value),removeItem:key=>stored.delete(key)};
const document={createElement:tag=>new Node(tag),createTextNode:text=>new Node('text',text)};
const run = vm.runInNewContext(`(function(panel,result,element,api){${source.slice(start,end)};return {choicesButton,choicesPanel};})`,{sessionStorage,setTimeout:fn=>fn(),document});
const {choicesButton, choicesPanel} = run(panel,{session_id:'session'},element,api);
(async()=>{
  await choicesButton.listeners.click();
  assert.equal(choicesPanel.children.length,2);
  assert.equal(choicesPanel.children[0].children[0].text,'Job location');
  assert.deepEqual(choicesPanel.children[0].children[1].children.map(item=>item.text),['Berlin','Lisbon']);
  assert.match(choicesPanel.children[0].children[2].text,/additional options may exist/);
  assert.match(choicesPanel.children[1].children[1].text,/not currently available/);
  assert.equal(choicesButton.disabled,false);
  const visual=choicesPanel.children[0].children[3], visualResult=choicesPanel.children[0].children[4];
  await visual.listeners.click();
  assert.match(visualResult.children[0].text,/local model observed the label/);
  assert.deepEqual(calls,['/api/form/custom-choices','/api/form/control-vision-pending','/api/form/custom-choice-vision-start','/api/form/control-vision-status']);
  assert.equal(stored.size,0);
  const optionRow=choicesPanel.children[0].children[1].children[0];
  await optionRow.children[0].listeners.click();
  assert.match(optionRow.children[1].textContent,/observed this option/);
  assert.equal(stored.size,0);
  const answer=choicesPanel.children[0].children[5].children[0],suggest=choicesPanel.children[0].children[6],advice=choicesPanel.children[0].children[7];
  assert.match(choicesPanel.children[0].children[5].text,/fact or preference, not instructions/);
  answer.value='I prefer Berlin';await suggest.listeners.click();
  assert.match(advice.children[0].text,/Berlin/);assert.match(advice.children[2].text,/does not verify your answer/);assert.equal(stored.size,0);
  assert.match(advice.children[1].text,/I prefer Berlin/);await advice.children[3].listeners.click();assert.match(optionRow.children[1].textContent,/observed this option/);
  const beforeExact=calls.filter(path=>path==='/api/form/custom-choice-advice-status').length;answer.value='Berlin';await suggest.listeners.click();assert.match(advice.children[0].text,/Berlin/);assert.equal(calls.filter(path=>path==='/api/form/custom-choice-advice-status').length,beforeExact);assert.equal(stored.size,0);
  answer.value='I prefer Lisbon';answer.listeners.input();assert.equal(advice.children.length,0);
  const actionCalls=[];
  const closed={choices:[{label:'Job location',selector:'#location',expanded:false,options:[]}],incomplete:false,snapshot_digest:'b'.repeat(64),can_open:true};
  const actionApi=async(path,body)=>{
    actionCalls.push(path);
    if(path==='/api/form/custom-choices')return closed;
    if(path==='/api/form/control-vision-pending'||path==='/api/form/custom-choice-action-pending')return {active:false};
    if(path==='/api/form/custom-choice-vision-start')return {task_id:'vision2'};
    if(path==='/api/form/control-vision-status')return {status:'complete',choice:true,session_id:'session',label:'Job location',comparison:'label_observed',capture_saved:true,result_saved:true,pending_finished:true};
    if(path==='/api/form/custom-choice-open'){assert.equal(body.reviewed,true);assert.equal(body.source_reviewed,true);assert.equal(body.visual_task_id,'vision2');return {task_id:'action'};}
    if(path==='/api/form/custom-choice-action-status')return {status:'complete',opened:true,record_saved:true,previous_fields_verified:true};
    throw new Error(`Unexpected request ${path}`);
  };
  const next=run(new Node('panel'),{session_id:'session'},element,actionApi);await next.choicesButton.listeners.click();
  const section=next.choicesPanel.children[0], visual2=section.children[3], result2=section.children[4];
  await visual2.listeners.click();
  const review=result2.children[1],open=result2.children[2];
  await open.listeners.click();assert.ok(!actionCalls.includes('/api/form/custom-choice-open'));
  review.children[0].checked=true;await open.listeners.click();
  assert.ok(actionCalls.includes('/api/form/custom-choice-open'));
  assert.ok(result2.children.some(item=>/Choice opened/.test(item.text)));
  assert.equal(stored.size,0);
  const selectCalls=[];
  const openInventory={choices:[{label:'Job location',selector:'#location',expanded:true,options:['Berlin']}],incomplete:false,snapshot_digest:'c'.repeat(64),can_open:true};
  const selectApi=async(path,body)=>{
    selectCalls.push(path);
    if(path==='/api/form/custom-choices')return openInventory;
    if(path==='/api/form/control-vision-pending'||path==='/api/form/custom-choice-action-pending')return {active:false};
    if(path==='/api/form/custom-choice-option-vision-start')return {task_id:'option'};
    if(path==='/api/form/custom-choice-selected-vision-start')return {task_id:'selected'};
    if(path==='/api/form/control-vision-status')return body.task_id==='selected'?{status:'complete',rank_task_id:'custom-choice-selected',session_id:'session',label:'Berlin',post_choice_review_finished:true}:{status:'complete',rank_task_id:'custom-choice-option',session_id:'session',label:'Berlin',comparison:'label_observed',capture_saved:true,result_saved:true,pending_finished:true};
    if(path==='/api/form/custom-choice-select'){assert.equal(body.visual_task_id,'option');return {task_id:'select-action'};}
    if(path==='/api/form/custom-choice-action-status')return {status:'complete',selected:true,option:'Berlin',record_saved:true,previous_fields_verified:true};
    throw new Error(`Unexpected request ${path}`);
  };
  const choosing=run(new Node('panel'),{session_id:'session'},element,selectApi);await choosing.choicesButton.listeners.click();
  const option=choosing.choicesPanel.children[0].children[1].children[0];await option.children[0].listeners.click();
  const actions=option.children[2],confirm=actions.children[0].children[0],select=actions.children[1];
  await select.listeners.click();assert.ok(!selectCalls.includes('/api/form/custom-choice-select'));
  confirm.checked=true;await select.listeners.click();assert.ok(selectCalls.includes('/api/form/custom-choice-select'));
  const verify=actions.children.find(item=>item.text==='Check selected value with local vision');await verify.listeners.click();
  assert.ok(actions.children.some(item=>/selected value was visually checked/.test(item.text)));
  assert.equal(stored.size,0);
  let oldDrained=false;
  const recoveryApi=async(path,body)=>{if(path==='/api/form/custom-choices')return {choices:[],snapshot_digest:'d'.repeat(64),pending_selected:{selector:'#location',option:'Berlin'}};if(path==='/api/form/control-vision-status' && body.task_id==='old-task'){oldDrained=true;return {status:'failed'};}return selectApi(path,body);};
  stored.set('waypoint-choice-selected-vision',JSON.stringify({task:'old-task',session:'old-session',selector:'#location'}));
  const recovery=run(new Node('panel'),{session_id:'session'},element,recoveryApi);await recovery.choicesButton.listeners.click();
  const recovered=recovery.choicesPanel.children[0];assert.match(recovered.children[0].text,/Berlin/);
  await recovered.children[1].listeners.click();assert.ok(recovered.children.some(item=>/selected value was visually checked/.test(item.text)));assert.equal(stored.size,0);assert.equal(oldDrained,true);
  const scrollCalls=[];
  const scrollApi=async(path,body)=>{
    scrollCalls.push(path);
    if(path==='/api/form/custom-choices')return openInventory;
    if(path==='/api/form/control-vision-pending'||path==='/api/form/custom-choice-action-pending')return {active:false};
    if(path==='/api/form/custom-choice-vision-start')return {task_id:'scroll-vision'};
    if(path==='/api/form/control-vision-status')return {status:'complete',choice:true,session_id:'session',label:'Job location',comparison:'label_observed',capture_saved:true,result_saved:true,pending_finished:true};
    if(path==='/api/form/custom-choice-scroll'){assert.equal(body.direction,'down');assert.equal(body.reviewed,true);return {task_id:'scroll-action'};}
    if(path==='/api/form/custom-choice-action-status')return {status:'complete',scrolled:true,record_saved:true,previous_fields_verified:true};
    throw new Error(`Unexpected request ${path}`);
  };
  const scrolling=run(new Node('panel'),{session_id:'session'},element,scrollApi);await scrolling.choicesButton.listeners.click();
  const scrollSection=scrolling.choicesPanel.children[0];await scrollSection.children[3].listeners.click();
  const scrollResult=scrollSection.children[4],scrollReview=scrollResult.children[1],down=scrollResult.children[3];
  await down.listeners.click();assert.ok(!scrollCalls.includes('/api/form/custom-choice-scroll'));
  scrollReview.children[0].checked=true;await down.listeners.click();assert.ok(scrollCalls.includes('/api/form/custom-choice-scroll'));
  assert.ok(scrollResult.children.some(item=>/List moved/.test(item.text)));
  let inventoryCalls=0;
  const resumedApi=async(path,body)=>{
    if(path==='/api/form/custom-choices'){inventoryCalls++;return {...openInventory,can_open:inventoryCalls>1};}
    if(path==='/api/form/control-vision-pending')return {active:false};
    if(path==='/api/form/custom-choice-vision-start')return {task_id:'after-scroll'};
    if(path==='/api/form/control-vision-status')return {status:'complete',choice:true,session_id:'session',label:'Job location',comparison:'label_observed',capture_saved:true,result_saved:true,pending_finished:true,post_choice_review_finished:true};
    throw new Error(`Unexpected request ${path}`);
  };
  const resumed=run(new Node('panel'),{session_id:'session'},element,resumedApi);await resumed.choicesButton.listeners.click();
  const resumedSection=resumed.choicesPanel.children[0];await resumedSection.children[3].listeners.click();
  assert.equal(inventoryCalls,2);assert.ok(resumedSection.children[4].children.some(item=>item.text==='Show later options'));
  let pagesRead=0;
  const many=Array.from({length:31},(_,i)=>({label:`Choice ${i+1}`,selector:`#choice-${i}`,expanded:false,options:[]}));
  const paged=run(new Node('panel'),{session_id:'session'},element,async(path)=>{assert.equal(path,'/api/form/custom-choices');pagesRead++;return {choices:many,incomplete:false,snapshot_digest:'e'.repeat(64)}});
  await paged.choicesButton.listeners.click();assert.equal(paged.choicesPanel.children[0].children[0].text,'Choice 1');
  let nav=paged.choicesPanel.children.at(-1);assert.match(nav.children[1].text,/Choices 1–15 of 31/);nav.children[2].click();await new Promise(resolve=>setImmediate(resolve));
  assert.equal(paged.choicesPanel.children[0].children[0].text,'Choice 16');nav=paged.choicesPanel.children.at(-1);nav.children[2].click();await new Promise(resolve=>setImmediate(resolve));
  assert.equal(paged.choicesPanel.children[0].children[0].text,'Choice 31');nav=paged.choicesPanel.children.at(-1);assert.equal(nav.children[2].disabled,true);nav.children[0].click();await new Promise(resolve=>setImmediate(resolve));assert.equal(paged.choicesPanel.children[0].children[0].text,'Choice 16');assert.equal(pagesRead,4);
  many.splice(10);nav=paged.choicesPanel.children.at(-1);nav.children[2].click();await new Promise(resolve=>setImmediate(resolve));assert.equal(paged.choicesPanel.children[0].children[0].text,'Choice 1');assert.equal(paged.choicesPanel.children.length,10);
  const empty=run(new Node('panel'),{session_id:'session'},element,async()=>({choices:[],incomplete:true,snapshot_digest:'f'.repeat(64)}));
  assert.equal(empty.choicesButton.text,'Inspect custom comboboxes');
  await empty.choicesButton.listeners.click();
  assert.match(empty.choicesPanel.children[0].text,/No visible custom comboboxes/);
  assert.match(empty.choicesPanel.children[0].text,/Other choice controls may still need manual review/);
  assert.ok(empty.choicesPanel.children.some(item=>/could not be inspected/.test(item.text)));
  console.log('Custom choice review UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
