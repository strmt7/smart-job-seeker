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
const inventory = {choices:[{label:'Job location',expanded:true,options:['Berlin','Lisbon'],options_incomplete:true},{label:'Department',expanded:false,options:[]}],incomplete:false};
const api = async (path, body) => { assert.equal(path, '/api/form/custom-choices'); assert.equal(body.session_id, 'session'); return inventory; };
const run = vm.runInNewContext(`(function(panel,result,element,api){${source.slice(start,end)};return {choicesButton,choicesPanel};})`);
const {choicesButton, choicesPanel} = run(panel,{session_id:'session'},element,api);
(async()=>{
  await choicesButton.listeners.click();
  assert.equal(choicesPanel.children.length,2);
  assert.equal(choicesPanel.children[0].children[0].text,'Job location');
  assert.deepEqual(choicesPanel.children[0].children[1].children.map(item=>item.text),['Berlin','Lisbon']);
  assert.match(choicesPanel.children[0].children[2].text,/Nothing was selected/);
  assert.match(choicesPanel.children[1].children[1].text,/not currently available/);
  assert.equal(choicesButton.disabled,false);
  console.log('Custom choice review UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
