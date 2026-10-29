const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf('let webSearchFollowing=');
const end = source.indexOf('\nlet webBatchRunning=', start);
assert.ok(start >= 0 && end > start, 'single-search recovery must exist');
const recovery = source.slice(start, end);

async function runFixture(saved, active) {
    const storage = new Map(saved ? [['waypoint-web-search', saved]] : []);
    const calls = [];
    const button = { disabled: false };
    const form = { addEventListener() {}, querySelector() { return button; } };
    const panel = { rows: [], replaceChildren(...rows) { this.rows = rows; }, append(row) { this.rows.push(row); } };
    const context = {
        document: { querySelector(selector) {
            if (selector === '#web-search') return form;
            if (selector === '#web-search button') return button;
            if (selector === '#web-search-results') return panel;
            throw Error(`Unexpected selector ${selector}`);
        } },
        sessionStorage: { getItem: key => storage.get(key) || null, setItem: (key, value) => storage.set(key, value), removeItem: key => storage.delete(key) },
        element: (_tag, text) => ({ text }), webCandidate: item => ({ text: item.title }),
        searchCoverage: () => 'fixture coverage', notify: message => { throw Error(message); },
        setTimeout: callback => callback(),
        api: async (path, body) => {
            calls.push([path, body]);
            if (path === '/api/web-search/pending') return { task_id: active };
            if (path === '/api/web-search/status') return { status: 'complete', data: { results: [{title: 'Fictional role'}] } };
            throw Error(`Unexpected request ${path}`);
        },
    };
    vm.runInNewContext(recovery, context);
    await new Promise(resolve => setImmediate(resolve));
    return { storage, calls, button, panel };
}

(async () => {
    const newer = await runFixture('old-task', 'new-task');
    assert.equal(newer.storage.get('waypoint-web-search'), 'new-task');
    assert.equal(newer.calls[1][1].task_id, 'new-task');
    assert.equal(newer.button.disabled, false);
    assert.ok(newer.panel.rows.some(row => row.text === 'Fictional role'));
    const replay = await runFixture('new-task', null);
    assert.equal(replay.calls[1][1].task_id, 'new-task');
    assert.equal(replay.storage.get('waypoint-web-search'), 'new-task');
    assert.ok(!replay.calls.some(([path]) => path === '/api/web-search/start'));
    console.log('Single-search recovery UI checks passed.');
})().catch(error => { console.error(error); process.exitCode = 1; });
