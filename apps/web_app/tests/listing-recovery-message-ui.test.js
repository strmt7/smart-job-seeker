const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const lines = source.split('\n');
const start = lines.findIndex(line => line.startsWith('async function offerListingRecovery('));
assert.ok(start >= 0);
const panel = {children: [], replaceChildren(...nodes) { this.children = nodes; }, append(...nodes) { this.children.push(...nodes); }};
const context = {document: {querySelector: () => panel}, api: async () => ({}), element: (tag, text) => ({tag, text}), candidateBatch: null};
const recover = vm.runInNewContext(`(${lines.slice(start, start + 2).join('\n')})`, context);
(async () => {
  await recover('Possible closure wording: Closed Positions. Verify the current status.');
  assert.equal(panel.children.length, 1);
  assert.match(panel.children[0].text, /Closed Positions/);
  await recover();
  assert.equal(panel.children.length, 0);
  console.log('Listing recovery preserves review messages.');
})().catch(error => { console.error(error); process.exitCode = 1; });
