const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const html = fs.readFileSync('apps/web_app/web/index.html', 'utf8');
const line = prefix => {
  const found = source.split('\n').find(item => item.startsWith(prefix));
  assert.ok(found, `Missing ${prefix}`);
  return found;
};
const limit = line('const FIT_BATCH_LIMIT');
const update = line('function updateSelection()');
const click = line("document.querySelector('#assess-selected').addEventListener");
const restore = line("try{const pending=JSON.parse(sessionStorage.getItem('waypoint-fit-batch')");
assert.match(html, /Assess up to 100 selected jobs sequentially/);

function context(size, saved = null) {
  const nodes = new Map();
  const listeners = {};
  const storage = new Map(saved ? [['waypoint-fit-batch', JSON.stringify(saved)]] : []);
  const ctx = {
    selectedJobs: new Set(Array.from({length:size}, (_, i) => `job-${i}`)),
    shortlisting: false, fitBatchRunning: false, autoFitRunning: false,
    fitBatch: null, runs: 0,
    document: {querySelector(selector) {
      if (!nodes.has(selector)) nodes.set(selector, {addEventListener: (_type, fn) => { listeners[selector] = fn; }});
      return nodes.get(selector);
    }},
    sessionStorage: {getItem: key => storage.get(key), removeItem: key => storage.delete(key)},
    saveFitBatch() {}, runFitBatch() { ctx.runs++; },
  };
  vm.runInNewContext(`${limit}\n${update}\n${click}`, ctx);
  return {ctx, nodes, listeners, storage};
}
const full = context(100);
full.ctx.updateSelection();
assert.equal(full.nodes.get('#assess-selected').disabled, false);
full.listeners['#assess-selected']();
assert.equal(full.ctx.fitBatch.ids.length, 100);
assert.equal(full.ctx.runs, 1);
const over = context(101);
over.ctx.updateSelection();
assert.equal(over.nodes.get('#assess-selected').disabled, true);
over.listeners['#assess-selected']();
assert.equal(over.ctx.runs, 0);
const saved = {ids:Array.from({length:100}, (_, i) => `job-${i}`), index:42, failed:0};
const resumed = context(0, saved);
vm.runInNewContext(restore, resumed.ctx);
assert.equal(resumed.ctx.fitBatch.index, 42);
assert.equal(resumed.ctx.runs, 1);
const oversized = context(0, {...saved, ids:[...saved.ids, 'job-100']});
vm.runInNewContext(restore, oversized.ctx);
assert.equal(oversized.ctx.fitBatch, null);
assert.equal(oversized.ctx.runs, 0);
assert.equal(oversized.storage.has('waypoint-fit-batch'), false);

(async () => {
  let active = 0, started = 0, maxActive = 0;
  const test = context(0).ctx;
  Object.assign(test, {
    fitBatch: {ids:saved.ids, index:0, failed:0, task:null, stop:false},
    fitBatchRunning:false, state:{jobs:[]}, renderFitBatch() {},
    refresh:async()=>{}, updateSelection(){}, scheduleAutomaticFit(){},
    setTimeout:fn=>fn(),
    api:async (path, body) => {
      if (path === '/api/fit/pending') return {active:false};
      if (path === '/api/fit/start') { active++; started++; maxActive=Math.max(maxActive,active); return {task_id:body.id}; }
      if (path === '/api/fit/status') { active--; return {status:'complete',saved:true}; }
      throw Error(`Unexpected endpoint ${path}`);
    },
  });
  vm.runInNewContext(line('async function runFitBatch()'), test);
  await test.runFitBatch();
  assert.equal(started, 100);
  assert.equal(maxActive, 1);
  assert.equal(test.fitBatch, null);
  console.log('Selected fit batch limit, recovery and sequential execution checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
