const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf('let webSearchFollowing=');
const end = source.indexOf('\nlet webBatchRunning=', start);
assert.ok(start >= 0 && end > start, 'single-search recovery must exist');
const recovery = source.slice(start, end);

async function runFixture(saved, active, results = [{title: 'Fictional role'}]) {
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
        element: (tag, text) => ({
            tag, text, children: [], events: {},
            append(...children) { this.children.push(...children); },
            addEventListener(name, fn) { this.events[name] = fn; },
        }),
        webCandidate: item => ({ text: item.title, candidate: true }),
        searchCoverage: () => 'fixture coverage', notify: message => { throw Error(message); },
        setTimeout: callback => callback(),
        api: async (path, body) => {
            calls.push([path, body]);
            if (path === '/api/web-search/pending') return { task_id: active };
            if (path === '/api/web-search/status') return { status: 'complete', data: { results } };
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
    const empty = await runFixture('empty-task', null, []);
    assert.ok(empty.panel.rows.some(row => row.text.includes('provider returned no candidates for this query')));
    assert.ok(empty.panel.rows.some(row => row.text.includes('remove search operators')));
    assert.ok(!newer.panel.rows.some(row => row.text.includes('remove search operators')));
    const many = await runFixture('large-task', null, Array.from({length: 620}, (_, i) => ({title: `Role ${i + 1}`})));
    assert.equal(many.panel.rows.filter(row => row.candidate).length, 40);
    const controls = many.panel.rows.find(row => row.tag === 'div');
    assert.equal(controls.children[0].disabled, true);
    assert.equal(controls.children[1].text, 'Candidates 1–40 of 620');
    controls.children[2].events.click();
    assert.ok(many.panel.rows.some(row => row.text === 'Role 41'));
    assert.ok(!many.panel.rows.some(row => row.text === 'Role 1'));
    console.log('Single-search recovery UI checks passed.');
})().catch(error => { console.error(error); process.exitCode = 1; });
