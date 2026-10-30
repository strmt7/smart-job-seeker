const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf("const compare=element('button','Compare with local AI')");
const end = source.indexOf('details.append(compare,comparison);', start);
assert.ok(start >= 0 && end > start);
const fragment = source.slice(start, end);
function element(tag, text='') { return {tag,textContent:text,children:[],listeners:{},addEventListener(event,fn){this.listeners[event]=fn;},replaceChildren(...nodes){this.children=nodes;},append(...nodes){this.children.push(...nodes);}}; }
let starts=0, polls=0;
async function api(path, body) {
  if(path.endsWith('compare-start')) { starts++; return {task_id:'fixture-task'}; }
  assert.equal(body.task_id,'fixture-task');
  polls++;
  if(polls===1) throw new Error('Connection lost');
  return {status:'complete',assessment:{verdict:'possibly_same',reason:'Fixture',first_excerpt:'',second_excerpt:''}};
}
const storage = new Map();
const sessionStorage = {getItem:key => storage.get(key) ?? null,setItem:(key,value) => storage.set(key,value),removeItem:key => storage.delete(key)};
const create = vm.runInNewContext(`(function(job,otherId){${fragment};return {compare,comparison};})`, {element,api,sessionStorage,setTimeout: fn => fn()});
(async()=>{
  const {compare,comparison}=create({id:'first'},'second');
  await compare.listeners.click();
  assert.equal(compare.textContent,'Retry comparison');
  assert.match(comparison.children[0].textContent,/Retry to recover/);
  const resumed = create({id:'first'},'second');
  assert.equal(resumed.compare.textContent,'Resume comparison');
  await resumed.compare.listeners.click();
  assert.equal(starts,1);
  assert.equal(polls,2);
  assert.equal(resumed.compare.textContent,'Compare with local AI');
  assert.match(resumed.comparison.children[0].textContent,/May be the same position/);
  assert.equal(storage.size,0);
  console.log('Duplicate comparison retry UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
