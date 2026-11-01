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
}
const panel = new Node('panel');
const element = (tag, text='') => new Node(tag, text);
const inventory = {choices:[{label:'Job location',selector:'#location',expanded:true,options:['Berlin','Lisbon'],options_incomplete:true},{label:'Department',selector:'#department',expanded:false,options:[]}],incomplete:false,snapshot_digest:'a'.repeat(64)};
const calls=[];
const api = async (path, body) => {
  calls.push(path);
  if(path==='/api/form/custom-choices'){assert.equal(body.session_id,'session');return inventory;}
  if(path==='/api/form/control-vision-pending')return {active:false};
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
  const recoveryApi=async(path,body)=>path==='/api/form/custom-choices'?{choices:[],snapshot_digest:'d'.repeat(64),pending_selected:{selector:'#location',option:'Berlin'}}:selectApi(path,body);
  const recovery=run(new Node('panel'),{session_id:'session'},element,recoveryApi);await recovery.choicesButton.listeners.click();
  const recovered=recovery.choicesPanel.children[0];assert.match(recovered.children[0].text,/Berlin/);
  await recovered.children[1].listeners.click();assert.ok(recovered.children.some(item=>/selected value was visually checked/.test(item.text)));
  console.log('Custom choice review UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
