const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf('for(const [integrity,label] of [[record.page_capture_integrity');
const end = source.indexOf('const reviewButton=', start);
assert.ok(start >= 0 && end > start);
const section = source.slice(start, end);
function messages(status) {
  const result = [];
  const entry = {append: item => result.push(item.text)};
  const element = (_tag, text, className) => ({text, className});
  vm.runInNewContext(section, {record:{page_capture_integrity:status,page_capture_available:true}, entry, element});
  return result.join(' ');
}
assert.match(messages('match'), /listed in the unsigned manifest/);
assert.match(messages('partial_match'), /viewer and extracted files were not checked/);
assert.match(messages('mismatch'), /differs from its saved fingerprints/);
console.log('Listing capture integrity UI checks passed.');
