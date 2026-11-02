const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const line = source.split('\n').find(value => value.startsWith('async function storageStatus()'));
assert.ok(line);
const path = { value: '', disabled: false };
const button = { disabled: false };
const form = { elements: { path }, querySelector: () => button };
const info = { textContent: '' };
let result = { active: 'isolated', configured: 'isolated', process_override: true };
const context = { api: async () => result, document: { querySelector: selector => selector === '#storage-info' ? info : form } };
const status = vm.runInNewContext(`(${line})`, context);
(async () => {
  await status();
  assert.match(info.textContent, /temporary workspace/);
  assert.equal(path.value, 'isolated');
  assert.equal(path.disabled, true);
  assert.equal(button.disabled, true);
  result = { active: 'normal', configured: 'normal', process_override: false };
  await status();
  assert.match(info.textContent, /Active: normal/);
  assert.equal(path.value, 'normal');
  assert.equal(path.disabled, false);
  assert.equal(button.disabled, false);
  console.log('Storage override UI checks passed.');
})().catch(error => { console.error(error); process.exitCode = 1; });
