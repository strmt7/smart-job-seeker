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
  return { create: id => window.WaypointSourceCopies.create({ id }, { api, element: (tag, text, style) => new Node(tag, text, style), onFinished }), tick: async () => { assert(timers.length, 'Expected a scheduled status poll'); timers.shift()(); await settle(); } };
}
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
