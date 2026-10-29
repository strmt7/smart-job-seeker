const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf('function searchPlanBinding(');
const end = source.indexOf("\ndocument.querySelector('#discovery-batch')", start);
assert.ok(start >= 0 && end > start);
const events = {};
const keyword = { value: 'Fictional engineer', dataset: { planDigest: 'digest', planQuery: 'Fictional engineer' }, addEventListener(name, fn) { events.keywordInput = fn; } };
const web = { value: '', dataset: {}, addEventListener() {} };
const form = { elements: { keyword }, addEventListener(name, fn) { events.submit = fn; } };
const calls = [];
const context = {
    document: { querySelector(selector) {
        if (selector === '#jobicy-search input[name=keyword]') return keyword;
        if (selector === '#web-search input[name=query]') return web;
        if (selector === '#jobicy-search') return form;
        throw Error(`Unexpected selector ${selector}`);
    } },
    startDiscovery: (targets, digest) => calls.push({ targets, digest }),
    notify: message => { throw Error(message); },
};
vm.runInNewContext(source.slice(start, end), context);
events.submit({ preventDefault() {}, currentTarget: form });
assert.equal(calls[0].digest, 'digest');
assert.equal(calls[0].targets[0].board, 'Fictional engineer');
keyword.value = 'Manually revised';
events.keywordInput();
events.submit({ preventDefault() {}, currentTarget: form });
assert.equal(calls[1].digest, null);
assert.equal(calls[1].targets[0].board, 'Manually revised');

(async () => {
    const webStart = source.indexOf("document.querySelector('#web-search').addEventListener('submit'");
    const webEnd = source.indexOf('\nasync function restoreWebSearch()', webStart);
    const bindingEnd = source.indexOf('\nfor(const selector', start);
    assert.ok(webStart >= 0 && webEnd > webStart && bindingEnd > start);
    const eventHandlers = {};
    const query = { value: 'Fictional engineer', dataset: { planDigest: 'digest', planQuery: 'Fictional engineer' } };
    const webButton = { disabled: false };
    const webForm = { elements: { query, provider: { value: 'https://provider.example' } }, querySelector() { return webButton; }, addEventListener(_name, fn) { eventHandlers.submit = fn; } };
    const panel = { replaceChildren() {} };
    let request;
    const webContext = {
        document: { querySelector(selector) { if (selector === '#web-search') return webForm; if (selector === '#web-search-results') return panel; throw Error(selector); } },
        element: (_tag, text) => ({ text }), sessionStorage: { setItem() {} },
        api: async (path, body) => { request = { path, body }; return { task_id: 'fixture' }; },
        followWebSearch: async () => {}, recoverPendingWebSearch: async () => {},
        notify: message => { throw Error(message); },
    };
    vm.runInNewContext(source.slice(start, bindingEnd) + '\n' + source.slice(webStart, webEnd), webContext);
    await eventHandlers.submit({ preventDefault() {}, currentTarget: webForm });
    assert.equal(request.path, '/api/web-search/start');
    assert.equal(request.body.plan_digest, 'digest');
    assert.equal(request.body.query, 'Fictional engineer');
    console.log('Search-plan binding UI checks passed.');
})().catch(error => { console.error(error); process.exitCode = 1; });
