const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const script = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const source = script.slice(script.indexOf('function sourceApplicationLink('), script.indexOf('function applicationCheckMessage('));
const calls = [];
const node = (tag, text) => ({tag, text, children: [], listeners: {}, append(...items) { this.children.push(...items); }, addEventListener(event, handler) { this.listeners[event] = handler; }});
const context = {URL, element: node, pendingApplicationCheck: () => null, api: async (path, body) => { calls.push({path, body}); return {task_id: 'task'}; }, sessionStorage: {setItem() {}}, followApplicationCheck: async () => {}, followFormInspection: async () => {}, notify() {}, document: {querySelector: () => ({checked: false})}};
vm.runInNewContext(source, context);
vm.runInNewContext(script.slice(script.indexOf('function applicationCheckMessage('), script.indexOf('function routeContactActions(')), context);
const job = {id: 'fictional', state: 'shortlisted', canonical_url: 'https://jobs.example/role'};
const link = context.sourceApplicationLink({source_application_url: 'https://jobs.example/role/application'}, job);
assert.equal(link.children[0].href, 'https://jobs.example/role/application');
assert.match(link.children[3].text, /Check its current status/);
for (const url of ['https://other.example/apply', 'https://jobs.example/apply?token=secret', 'javascript:alert(1)', 'https://person@jobs.example/apply']) {
  assert.equal(context.sourceApplicationLink({source_application_url: url}, job), null);
}
const check = {current: true, sourceRoute: true, listingUrl: job.canonical_url, url: 'https://jobs.example/role/application', text: 'Fictional result'};
assert.equal(context.applicationCheckMessage(check, job, {source_application_url: check.url}), 'Fictional result');
assert.match(context.applicationCheckMessage(check, job, {source_application_url: 'https://jobs.example/role/new'}), /out of date/);
assert.match(context.applicationCheckMessage(check, {canonical_url: 'https://jobs.example/other'}, {source_application_url: check.url}), /out of date/);
Promise.all([link.children[1].listeners.click(), link.children[2].listeners.click()]).then(() => {
  assert.equal(calls[0].path, '/api/application-check/start');
  assert.equal(calls[0].body.source_application, true);
  assert.equal(calls[1].path, '/api/form/inspect-start');
  assert.equal(calls[1].body.source_application, true);
  assert.equal(calls[1].body.require_no_session, true);
}).catch(error => { console.error(error); process.exitCode = 1; });
