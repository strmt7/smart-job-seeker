const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf('function renderSearchPlan(');
const end = source.indexOf("\ndocument.querySelector('#jobicy-search')", start);
assert.ok(start >= 0 && end > start, 'search planning UI must exist');

async function fixture(saved, active) {
    const storage = new Map(saved ? [['waypoint-search-plan', saved]] : []);
    const calls = [];
    const button = { disabled: false, addEventListener() {} };
    const panel = { rows: [], replaceChildren(...rows) { this.rows = rows; }, append(row) { this.rows.push(row); } };
    const context = {
        document: { querySelector(selector) {
            if (selector === '#plan-search') return button;
            if (selector === '#search-phrases') return panel;
            throw Error(`Unexpected selector ${selector}`);
        } },
        sessionStorage: { getItem: key => storage.get(key) || null, setItem: (key, value) => storage.set(key, value), removeItem: key => storage.delete(key) },
        element: (_tag, text) => ({ text, children: [], append(...children) { this.children.push(...children); }, addEventListener() {} }),
        notify: message => { throw Error(message); }, setTimeout: callback => callback(),
        api: async (path, body) => {
            calls.push([path, body]);
            if (path === '/api/search-plan/pending') return { task_id: active };
            if (path === '/api/search-plan/status') return { status: 'complete', queries: ['Fictional engineer'] };
            throw Error(`Unexpected request ${path}`);
        },
    };
    vm.runInNewContext(source.slice(start, end), context);
    await new Promise(resolve => setImmediate(resolve));
    return { storage, calls, button, panel };
}

(async () => {
    const newer = await fixture('old-plan', 'new-plan');
    assert.equal(newer.storage.get('waypoint-search-plan'), 'new-plan');
    assert.equal(newer.calls[1][1].task_id, 'new-plan');
    assert.equal(newer.button.disabled, false);
    assert.ok(newer.panel.rows.some(row => row.children?.some(child => child.text === 'Fictional engineer')));
    const replay = await fixture('new-plan', null);
    assert.equal(replay.calls[1][1].task_id, 'new-plan');
    assert.ok(!replay.calls.some(([path]) => path === '/api/search-plan/start'));
    console.log('Search-plan recovery UI checks passed.');
})().catch(error => { console.error(error); process.exitCode = 1; });
