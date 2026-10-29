// Exercise the actual UI comparison helper without starting the app or a browser.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../web/app.js'), 'utf8');
const start = source.indexOf('function visualComparisonStatus(');
const end = source.indexOf('function showAutomaticFit(', start);
assert.ok(start >= 0 && end > start, 'UI comparison helper must be present');
const status = vm.runInNewContext(`${source.slice(start, end)}; visualComparisonStatus`);
const field = {label: 'Email', expected: 'candidate@example.invalid'};
const observed = {label: 'email', visible_value: field.expected};
assert.equal(status(field, [field], [observed], true), 'transcription matches reviewed answer');
assert.equal(status(field, [field], [observed], false), 'capture changed or freshness could not be confirmed');
assert.equal(status(field, [field], [], true), 'not uniquely observed');
assert.equal(status(field, [field], [observed, observed], true), 'not uniquely observed');
assert.equal(status(field, [field, field], [observed], true), 'not uniquely observed');
assert.equal(status(field, [field], [{label: 'Email', visible_value: 'other'}], true), 'transcription differs from reviewed answer');
const multiline = {label: 'First name', expected: 'Example'};
assert.equal(status(multiline, [multiline], [{label: ' FIRST\n  NAME ', visible_value: 'Example'}], true), 'transcription matches reviewed answer');
assert.equal(status(multiline, [multiline, {label: 'FIRST\nNAME', expected: 'Example'}], [{label: 'First name', visible_value: 'Example'}], true), 'not uniquely observed');
assert.equal(status(multiline, [multiline], [{label: 'FIRST NAME', visible_value: 'Example'}, {label: 'First\nname', visible_value: 'Example'}], true), 'not uniquely observed');
assert.equal(status(multiline, [multiline], [{label: '\ufeffFirst name', visible_value: 'Example'}], true), 'not uniquely observed');
console.log('Visual comparison UI checks passed.');
