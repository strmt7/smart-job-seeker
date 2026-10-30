const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const line = source.split('\n').find(value => value.startsWith('function renderProfileSummary('));
function node(tag, text = '') { return {tag, textContent:text, childNodes:[], append(...items){this.childNodes.push(...items);}, replaceChildren(){this.childNodes=[];}}; }
const summary = node('div'), preferences = node('div');
const context = {element: (tag,text) => node(tag,text), document:{querySelector: selector => selector === '#profile-summary' ? summary : preferences}};
const render = vm.runInNewContext(`(${line})`, context);
render({full_name:'Example, Avery', first_name:'Avery', last_name:'Example'});
assert.equal(summary.childNodes[0].childNodes[1].textContent, 'Example, Avery');
render({first_name:'Avery', last_name:'Example'});
assert.equal(summary.childNodes[0].childNodes[1].textContent, 'Avery Example');
assert.ok(fs.readFileSync('apps/web_app/web/index.html', 'utf8').includes('name="full_name" autocomplete="name"'));
console.log('Profile name UI checks passed.');
