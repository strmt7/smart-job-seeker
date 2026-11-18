const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const line = source.split('\n').find(item => item.startsWith("document.querySelector('#web-search-batch').addEventListener('submit'"));
const pauseStart = source.indexOf('function formatWebProviderPause(');
const pauseEnd = source.indexOf('async function followWebSearch(', pauseStart);
assert.ok(line && pauseStart >= 0 && pauseEnd > pauseStart);

async function run(saved, retry = null, edit = false, pause = null) {
 const notices = [], button = {disabled: false}, status = {textContent: ''};
 const form = {elements: {queries: {value: 'fictional role'}, pages: {value: '2'}}, querySelector: () => button};
 let submit, payload, seenBatch;
 form._waypointRetry = retry;
 if (edit) form.elements.queries.value = 'changed role';
 const context = {
  document: {querySelector: selector => {
   if (selector === '#web-search-batch') return {addEventListener: (_event, fn) => {submit = fn;}};
   if (selector === '#web-search-batch-status') return status;
   return {elements: {provider: {value: 'http://127.0.0.1:8080/'}}};
  }},
  webBatchRunning: false, webBatchPage: 0, searchPlanBinding: () => null,
  notify: message => notices.push(message),
  api: async (_path, body) => {
   payload = body;
   return pause ? {status: 'deferred', provider_pause: pause} : {task_id: 'fixture', checkpoint_saved: saved};
  },
  followWebBatch: async batch => {seenBatch = batch;},
 };
 vm.runInNewContext(source.slice(pauseStart, pauseEnd) + line, context);
 await submit({preventDefault() {}, currentTarget: form});
 assert.equal(button.disabled, false);
 return {notices, payload, seenBatch, retryAfter: form._waypointRetry, status: status.textContent};
}

(async () => {
 assert.match((await run(false)).notices[0], /Restart recovery is unavailable/);
 assert.equal((await run(true)).notices.length, 0);
 const retry = {
  queries: 'fictional role', pages: '2', plan: [{query: 'fictional role', page: 2}],
  priorGroups: [{query: 'old', page: 1, data: {results: []}}], sourceTask: 'previous-task',
 };
 const prepared = await run(true, retry);
 assert.equal(JSON.stringify(prepared.payload.page_plan), JSON.stringify(retry.plan));
 assert.equal(prepared.payload.retry_source_task, 'previous-task');
 assert.equal(JSON.stringify(prepared.seenBatch.priorGroups), JSON.stringify(retry.priorGroups));
 const edited = await run(true, retry, true);
 assert.equal(edited.payload.page_plan, undefined);
 assert.equal(edited.seenBatch.priorGroups.length, 0);

 const retry_at = Math.ceil(Date.now() / 1000) + 120;
 const held = await run(true, retry, false, {http_status: 429, retry_at, from_retry_after: true});
 assert.equal(held.seenBatch, undefined);
 assert.equal(held.retryAfter, retry);
 assert.match(held.status, /rate limiting.*Retry after/);
 const outage = await run(true, retry, false, {http_status: 503, retry_at, from_retry_after: false});
 assert.match(outage.status, /temporarily unavailable.*Retry after/);
 console.log('Web-search startup checkpoint and provider pause UI checks passed.');
})().catch(error => {console.error(error); process.exitCode = 1;});
