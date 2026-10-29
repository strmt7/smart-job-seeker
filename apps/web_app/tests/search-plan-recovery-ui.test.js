const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf('function renderSearchPlan(');
const end = source.indexOf("\ndocument.querySelector('#jobicy-search')", start);
assert.ok(start >= 0 && end > start, 'search planning UI must exist');

async function fixture(saved, active, current = true) {
    const storage = new Map(saved ? [['waypoint-search-plan', saved]] : []);
    const calls = [];
    const button = { disabled: false, addEventListener() {} };
    const field = { value: '', dataset: {}, closest() { return null; }, focus() {}, addEventListener() {} };
    const panel = { rows: [], replaceChildren(...rows) { this.rows = rows; }, append(row) { this.rows.push(row); } };
    const context = {
        document: { querySelector(selector) {
            if (selector === '#plan-search') return button;
            if (selector === '#search-phrases') return panel;
            if (selector === '#web-search input[name=query]') return field;
            if (selector === '#jobicy-search input[name=keyword]') return field;
            throw Error(`Unexpected selector ${selector}`);
        } },
        sessionStorage: { getItem: key => storage.get(key) || null, setItem: (key, value) => storage.set(key, value), removeItem: key => storage.delete(key) },
        element: (_tag, text) => ({ text, children: [], events: {}, append(...children) { this.children.push(...children); }, addEventListener(name, handler) { this.events[name] = handler; } }),
        notify: message => { throw Error(message); }, setTimeout: callback => callback(),
        api: async (path, body) => {
            calls.push([path, body]);
            if (path === '/api/search-plan/pending') return { task_id: active };
            if (path === '/api/search-plan/status') return { status: 'complete', queries: ['Fictional engineer'], input_digest: 'fixture-digest' };
            if (path === '/api/search-plan/current') return { current };
            throw Error(`Unexpected request ${path}`);
        },
    };
    vm.runInNewContext(source.slice(start, end), context);
    await new Promise(resolve => setImmediate(resolve));
    return { storage, calls, button, panel, field };
}

(async () => {
    const newer = await fixture('old-plan', 'new-plan');
    assert.equal(newer.storage.get('waypoint-search-plan'), 'new-plan');
    assert.equal(newer.calls[1][1].task_id, 'new-plan');
    assert.equal(newer.button.disabled, false);
    assert.ok(newer.panel.rows.some(row => row.children?.some(child => child.text === 'Fictional engineer')));
    const currentPhrase = newer.panel.rows.find(row => row.children?.some(child => child.text === 'Fictional engineer'));
    await currentPhrase.children.find(child => child.text === 'Use on web').events.click();
    assert.equal(newer.field.value, 'Fictional engineer');
    assert.equal(newer.field.dataset.planDigest, 'fixture-digest');
    const bindingStart = source.indexOf('function searchPlanBinding(');
    const bindingEnd = source.indexOf('\nfor(const selector', bindingStart);
    assert.ok(bindingStart >= 0 && bindingEnd > bindingStart);
    const binding = vm.runInNewContext(`(${source.slice(bindingStart, bindingEnd)})`);
    assert.equal(binding(newer.field), 'fixture-digest');
    newer.field.value = 'Manually revised search';
    assert.equal(binding(newer.field), null);
    assert.equal(newer.field.dataset.planDigest, undefined);
    const replay = await fixture('new-plan', null);
    assert.equal(replay.calls[1][1].task_id, 'new-plan');
    assert.ok(!replay.calls.some(([path]) => path === '/api/search-plan/start'));
    const stale = await fixture('new-plan', null, false);
    const phrase = stale.panel.rows.find(row => row.children?.some(child => child.text === 'Fictional engineer'));
    await phrase.children.find(child => child.text === 'Use on web').events.click();
    assert.equal(stale.field.value, '');
    assert.ok(stale.panel.rows.some(row => row.text.includes('changed')));
    assert.equal(stale.storage.get('waypoint-search-plan'), undefined);
    console.log('Search-plan recovery UI checks passed.');
})().catch(error => { console.error(error); process.exitCode = 1; });
