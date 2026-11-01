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
  if(path==='/api/form/custom-choice-vision-start'){assert.equal(body.selector,'#location');assert.equal(body.snapshot_digest,inventory.snapshot_digest);return {task_id:'vision'};}
  if(path==='/api/form/control-vision-status')return {status:'complete',choice:true,session_id:'session',label:'Job location',comparison:'label_observed',capture_saved:true,result_saved:true};
  throw new Error(`Unexpected request ${path}`);
};
const stored = new Map();
const sessionStorage={getItem:key=>stored.get(key),setItem:(key,value)=>stored.set(key,value),removeItem:key=>stored.delete(key)};
const run = vm.runInNewContext(`(function(panel,result,element,api){${source.slice(start,end)};return {choicesButton,choicesPanel};})`,{sessionStorage,setTimeout:fn=>fn()});
const {choicesButton, choicesPanel} = run(panel,{session_id:'session'},element,api);
(async()=>{
  await choicesButton.listeners.click();
  assert.equal(choicesPanel.children.length,2);
  assert.equal(choicesPanel.children[0].children[0].text,'Job location');
  assert.deepEqual(choicesPanel.children[0].children[1].children.map(item=>item.text),['Berlin','Lisbon']);
  assert.match(choicesPanel.children[0].children[2].text,/Nothing was selected/);
  assert.match(choicesPanel.children[1].children[1].text,/not currently available/);
  assert.equal(choicesButton.disabled,false);
  const visual=choicesPanel.children[0].children[3], visualResult=choicesPanel.children[0].children[4];
  await visual.listeners.click();
  assert.match(visualResult.children[0].text,/local model observed the label/);
  assert.deepEqual(calls,['/api/form/custom-choices','/api/form/control-vision-pending','/api/form/custom-choice-vision-start','/api/form/control-vision-status']);
  assert.equal(stored.size,0);
  console.log('Custom choice review UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
