const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../scripts/qualify-layout.cjs'), 'utf8');
const start = source.indexOf('async function visibleBox(');
const end = source.indexOf('\nasync function bounded(', start);
assert.ok(start >= 0 && end > start);
const context = vm.createContext({ bounded: operation => operation, phase: null });
vm.runInContext(source.slice(start, end), context);

(async () => {
  const calls = [];
  const box = { x: 2, y: 3, width: 40, height: 50 };
  let ready = false;
  const locator = {
    async waitFor(options) {
      assert.equal(options.state, 'visible');
      await Promise.resolve();
      ready = true;
      calls.push('visible');
    },
    async scrollIntoViewIfNeeded() {
      assert.ok(ready);
      calls.push('stable');
    },
    async boundingBox() {
      assert.deepEqual(calls, ['visible', 'stable']);
      return box;
    },
  };
  assert.equal(await context.visibleBox(locator, 'Delayed dialog'), box);
  assert.equal(context.phase, 'Delayed dialog');
  await assert.rejects(context.visibleBox({
    async waitFor() { throw Error('still hidden'); },
    async boundingBox() { throw Error('must not read hidden geometry'); },
  }, 'Hidden dialog'), /still hidden/);
  for (const invalid of [null, { ...box, x: NaN }, { ...box, width: 0 }]) {
    await assert.rejects(context.visibleBox({
      async waitFor() {},
      async scrollIntoViewIfNeeded() {},
      async boundingBox() { return invalid; },
    }, 'Broken pane'), /Broken pane has no usable visible geometry/);
  }
  console.log('Layout qualification waits for actionable geometry and retains failures');
})().catch(error => { console.error(error); process.exitCode = 1; });
