const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const helper = source.split('\n').find(line => line.startsWith('async function reopenVisibleFormReview('));
assert.ok(helper);
const priorFillLine = source.split('\n').find(line => line.startsWith('function formMayHavePriorFill('));
assert.ok(priorFillLine);
const priorFill = vm.runInNewContext(`(${priorFillLine})`);
assert.equal(priorFill({fill_attempted:false}),false);
assert.equal(priorFill({fill_attempted:true}),true);
assert.equal(priorFill({}),true);

async function exercise(failure) {
  const calls = [], storage = new Map();
  let closed = false;
  const context = {
    api: async (path, body) => {
      calls.push([path, JSON.parse(JSON.stringify(body))]);
      if (path === failure) throw new Error('fixture failure');
      return path.endsWith('inspect-start') ? {task_id: 'task'} : {};
    },
    sessionStorage: {setItem: (key, value) => storage.set(key, value)},
    followFormInspection: async (task, job) => { calls.push(['follow', task, job]); },
  };
  vm.createContext(context);
  const reopen = vm.runInContext(`(${helper})`, context);
  try { await reopen('job', 'session', () => { closed = true; }); }
  catch (error) { assert.equal(error.message, 'fixture failure'); }
  return {calls, storage, closed};
}

(async () => {
  const ok = await exercise(null);
  assert.deepEqual(ok.calls, [
    ['/api/form/close', {session_id: 'session'}],
    ['/api/form/inspect-start', {id: 'job', visible: true, require_no_session: true}],
    ['follow', 'task', 'job'],
  ]);
  assert.equal(ok.closed, true);
  assert.deepEqual(JSON.parse(ok.storage.get('waypoint-form-inspection')), {task: 'task', job: 'job'});
  const closeFailed = await exercise('/api/form/close');
  assert.equal(closeFailed.closed, false);
  assert.equal(closeFailed.calls.length, 1);
  const startFailed = await exercise('/api/form/inspect-start');
  assert.equal(startFailed.closed, true);
  assert.equal(startFailed.storage.size, 0);
  assert.ok(source.includes('unsaved site changes and unsent answers in this panel may be lost'));
  assert.ok(source.includes('if(fillInProgress || visualButton.disabled)'));
  const start = source.indexOf('if(formRequiresVisibleBrowser(result)){');
  const end = source.indexOf('\n const image=', start);
  assert.ok(start >= 0 && end > start);
  const nodes = [], actions = [], fakePanel = {disabled: false, append: (...items) => nodes.push(...items)};
  const display = {
    result: {visible: false, fields: [], manual_review: ['overlay'], session_id: 'session'},
    job: {id: 'job'}, panel: fakePanel, root: {append: item => nodes.push(item)},
    element: (tag, text) => ({tag, text, setAttribute() {}, addEventListener: (_event, handler) => { if (tag === 'button') display.click = handler; }}),
    formRequiresVisibleBrowser: () => true, fillInProgress: false, fillCompleted: false, fillAttempted: false,
    visualButton: {disabled: false}, activeReviewSession: 'session', sessionTimer: 7,
    notify: text => actions.push(['notify', text]), renderReviewQueue: () => actions.push(['queue']),
    window: {confirm: text => { actions.push(['confirm', text]); return false; }},
    clearInterval: timer => actions.push(['clear', timer]), recoverReviewSession: async () => {},
    reopenVisibleFormReview: async (job, session, onClosed) => { actions.push(['reopen', job, session]); onClosed(); },
  };
  vm.createContext(display);
  vm.runInContext(source.slice(start, end), display);
  display.fillInProgress = true;
  await display.click();
  assert.ok(!actions.some(item => item[0] === 'reopen'));
  display.fillInProgress = false;
  await display.click();
  assert.ok(actions.some(item => item[0] === 'confirm' && item[1].includes('unsent answers')));
  assert.ok(!actions.some(item => item[0] === 'reopen'));
  display.fillCompleted = true;
  await display.click();
  assert.ok(actions.some(item => item[0] === 'confirm' && item[1].includes('unsaved site changes')));
  assert.ok(!actions.some(item => item[0] === 'reopen'));
  display.fillCompleted = false;
  display.fillAttempted = true;
  const confirmationsBefore = actions.filter(item => item[0] === 'confirm').length;
  await display.click();
  assert.equal(actions.filter(item => item[0] === 'confirm').length, confirmationsBefore + 1);
  assert.ok(!actions.some(item => item[0] === 'reopen'));
  display.window.confirm = () => true;
  await display.click();
  assert.ok(actions.some(item => item[0] === 'reopen' && item[1] === 'job' && item[2] === 'session'));
  assert.equal(display.activeReviewSession, null);
  assert.equal(fakePanel.disabled, true);
  assert.ok(nodes.some(item => item.text?.includes('unsaved site changes')));
  display.reopenVisibleFormReview = async (_job, _session, onClosed) => { onClosed(); throw new Error('start failed'); };
  display.activeReviewSession = 'session';
  await display.click();
  assert.ok(nodes.some(item => item.textContent?.includes('Visible inspection did not complete')));
  const closeLine = source.split('\n').find(line => line.startsWith(" const close=element('button','Close review session')"));
  assert.ok(closeLine);
  const closeCalls = [];
  const closeContext = {
    element: (tag, text) => ({tag, text, setAttribute() {}, addEventListener: (_event, handler) => { if (tag === 'button') closeContext.click = handler; }}),
    panel: {isConnected: true, append() {}}, root: {append() {}}, result: {session_id: 'session'},
    fillInProgress: true, visualButton: {disabled: false}, activeReviewSession: 'session', sessionTimer: 7,
    notify: text => closeCalls.push(['notice', text]), renderReviewQueue() {}, clearInterval() {},
    window: {confirm: text => { closeCalls.push(['confirm', text]); return false; }},
    api: async (...args) => { closeCalls.push(['api', ...args]); },
  };
  vm.createContext(closeContext);
  vm.runInContext(closeLine, closeContext);
  await closeContext.click();
  assert.ok(!closeCalls.some(call => call[0] === 'confirm' || call[0] === 'api'));
  closeContext.fillInProgress = false;
  await closeContext.click();
  assert.ok(closeCalls.some(call => call[0] === 'confirm' && call[1].includes('unsent answers')));
  assert.ok(!closeCalls.some(call => call[0] === 'api'));
  closeContext.window.confirm = () => true;
  await closeContext.click();
  assert.ok(closeCalls.some(call => call[0] === 'api' && call[1] === '/api/form/close'));
  assert.equal(closeContext.activeReviewSession, null);
  console.log('Visible form handoff checks passed.');
})().catch(error => { console.error(error); process.exitCode = 1; });
