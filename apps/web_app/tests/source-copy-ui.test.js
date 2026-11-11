'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
class Node {
  constructor(tag, text = '', className = '') { Object.assign(this, { tag, textContent: text, className, children: [], listeners: {}, attributes: {}, isConnected: true }); this.classList = { add: name => { this.className += ' ' + name; } }; }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = nodes; }
  setAttribute(key, value) { this.attributes[key] = value; }
  addEventListener(type, listener) { this.listeners[type] = listener; }
}
function find(root, predicate) { if (predicate(root)) return root; for (const node of root.children) { const result = find(node, predicate); if (result) return result; } }
const button = (root, text) => find(root, node => node.tag === 'button' && node.textContent === text);
const status = root => find(root, node => node.attributes.role === 'status');
const settle = () => new Promise(resolve => setImmediate(resolve));
function harness(api, onFinished) {
  const timers = [], window = {};
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../web/source-copy.js'), 'utf8'), { window, document: { createElement: tag => new Node(tag) }, setTimeout: callback => { timers.push(callback); } });
  return { preview: (preview, options) => window.WaypointSourceCopies.previewView(preview, (tag, text, style) => new Node(tag, text, style), options), create: id => window.WaypointSourceCopies.create({ id }, { api, element: (tag, text, style) => new Node(tag, text, style), onFinished }), tick: async () => { assert(timers.length, 'Expected a scheduled status poll'); timers.shift()(); await settle(); } };
}
(async () => {
  const digest = 'a'.repeat(64);
  const preview = { views: [
    {kind:'page',label:'Page',image:'0001.png',sha256:digest,fields:[{label:'Parent name'}]},
    {kind:'frame',label:'Qualifications',image:'0002.png',sha256:digest,fields:[{label:'Child answer',required:true}]},
  ] };
  let release;
  const panel = harness(async () => {}).preview(preview,{jobId:'job',attemptId:'attempt',api:async (_route,body) => {
    if(body.name==='0001.png') return new Promise(resolve => { release=resolve; });
    return {name:body.name,sha256:'b'.repeat(64),image_base64:'fixture'};
  }});
  const select = find(panel,node => node.tag==='select');
  const show = button(panel,'View image');
  const first = show.listeners.click();
  select.value='1'; select.listeners.change();
  release({name:'0001.png',sha256:digest,image_base64:'fixture'});
  await first;
  assert.equal(find(panel,node => node.tag==='img'),undefined,'A previous selection cannot replace the current view');
  assert(find(panel,node => node.textContent==='Child answer · Required'));
  await show.listeners.click();
  assert(find(panel,node => node.textContent==='The saved image differs from its record.'));
  assert.equal(find(panel,node => node.tag==='img'),undefined);

  const rehearsal = {...preview, rehearsal: {scope:'fictional_text_only', fields:[
    {view:2,label:'Child answer',status:'visual_unavailable',image:'0006.png',sha256:digest},
    {view:2,label:'Unclear field',status:'visual_review_failed'},
    {view:2,label:'Clear field',status:'visually_matched'},
  ]}};
  const tested = harness(async () => {}).preview(rehearsal,{jobId:'job',attemptId:'test',api:async (_route,body) => {
    assert.equal(body.name,'0006.png');
    return {name:body.name,sha256:digest,image_base64:'fixture'};
  }});
  assert.equal(find(tested,node => node.textContent==='View field image'),undefined);
  const testedSelect = find(tested,node => node.tag==='select');
  testedSelect.value='1'; testedSelect.listeners.change();
  assert(find(tested,node => node.textContent==='Child answer · Text verified in form · visual review unavailable'));
  assert(find(tested,node => node.textContent==='Unclear field · Text verified in form · image review failed'));
  assert(find(tested,node => node.textContent==='Clear field · Text and image match'));
  await button(tested,'View field image').listeners.click();
  assert(find(tested,node => node.tag==='img'));
  const choice = harness(async () => {}).preview({
    ...preview, rehearsal: {scope:'fictional_saved_form', fields:[
      {view:2,kind:'select',label:'Country',status:'visually_matched'},
      {view:2,kind:'radio',label:'City',status:'visual_unavailable'},
    ]},
  }, {jobId:'job',attemptId:'test',api:async () => {}});
  const choiceSelect = find(choice,node => node.tag==='select');
  choiceSelect.value='1'; choiceSelect.listeners.change();
  assert(find(choice,node => node.textContent==='Country · Selection and image match'));
  assert(find(choice,node => node.textContent==='City · Selection verified in form · visual review unavailable'));
})().catch(error=>{console.error(error);process.exitCode=1;});

(async () => {
  let finish, starts = 0, pending = 0;
  const h = harness(async route => {
    if (route.endsWith('/pending')) { pending++; return { active: false }; }
    if (route.endsWith('/start')) { starts++; return { task_id: 'copy-a' }; }
    if (route.endsWith('/status')) return { task_id: 'copy-a', job_id: 'a', status: 'complete', attempt_id: 'record-a', result: {} };
    throw Error('Unexpected route');
  }, () => new Promise(resolve => { finish = resolve; }));
  const first = h.create('a'), second = h.create('b');
  await settle(); assert.equal(pending, 1, 'Concurrent mounts share recovery');
  const click = button(first, 'Save page copy').listeners.click();
  await settle();
  assert.equal(starts, 1); assert(finish);
  assert.equal(button(first, 'Save another copy').disabled, true, 'Completion refresh keeps start disabled');
  assert.equal(button(second, 'Save page copy').disabled, true);
  await button(second, 'Save page copy').listeners.click(); assert.equal(starts, 1);
  finish(); await click;
  assert.equal(button(first, 'Save another copy').disabled, false);
  assert.match(status(first).textContent, /Page copy incomplete/);
  assert.match(status(first).textContent, /Check the saved record/);
  assert.doesNotMatch(status(first).textContent, /0 resources/);

  let cancelled = false;
  const active = harness(async route => {
    if (route.endsWith('/pending')) return { active: true, task_id: 'copy-b', job_id: 'b' };
    if (route.endsWith('/cancel')) throw Error('Stop request could not reach the server.');
    if (route.endsWith('/status')) return cancelled ? { status: 'cancelled', task_id: 'copy-b', job_id: 'b' } : { status: 'running', task_id: 'copy-b', job_id: 'b', phase: 'resources' };
    throw Error('Unexpected route');
  });
  const own = active.create('b'), other = active.create('c'); await settle();
  assert.equal(find(own, node => node.tag === 'progress').hidden, false);
  assert.equal(find(other, node => node.tag === 'progress').hidden, true);
  await button(other, 'Stop current copy').listeners.click();
  assert.match(status(other).textContent, /Stop request could not reach the server/);
  assert.equal(button(other, 'Stop current copy').disabled, false);
  cancelled = true; await active.tick();
  assert.match(status(own).textContent, /^Stopped\./);
  assert.equal(button(own, 'Save another copy').disabled, false);

  const mismatch = harness(async route => route.endsWith('/pending') ? { active: true, task_id: 'expected', job_id: 'a' } : { task_id: 'wrong', job_id: 'a', status: 'complete' });
  const refused = mismatch.create('a'); await settle();
  assert.match(status(refused).textContent, /does not match this job/);
  assert.doesNotMatch(status(refused).textContent, /Page copy saved/);
  const html = fs.readFileSync(path.join(__dirname, '../web/index.html'), 'utf8');
  assert(html.indexOf('/source-copy.js') < html.indexOf('/app.js') && html.includes('/source-copy.js'));
  console.log('Source-copy shared recovery, refresh race, unknown accounting, cancellation errors and result binding passed.');
})().catch(error => { console.error(error); process.exitCode = 1; });
