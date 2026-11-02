const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const script = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const source = script.slice(script.indexOf('function sourceApplicationLink('), script.indexOf('function applicationCheckMessage('));
const node = (tag, text) => ({tag, text, children: [], append(...items) { this.children.push(...items); }});
const context = {URL, element: node};
vm.runInNewContext(source, context);
const job = {canonical_url: 'https://jobs.example/role'};
const link = context.sourceApplicationLink({source_application_url: 'https://jobs.example/role/application'}, job);
assert.equal(link.children[0].href, 'https://jobs.example/role/application');
assert.match(link.children[1].text, /not been checked/);
for (const url of ['https://other.example/apply', 'https://jobs.example/apply?token=secret', 'javascript:alert(1)', 'https://person@jobs.example/apply']) {
  assert.equal(context.sourceApplicationLink({source_application_url: url}, job), null);
}
