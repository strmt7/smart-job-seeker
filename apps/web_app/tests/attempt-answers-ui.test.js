const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf('function renderAttemptAnswers(');
const end = source.indexOf('\nfunction ', start);
assert.ok(start >= 0 && end > start);
function element(tag, text = '') { return {tag, text, children: [], append(...items) { this.children.push(...items); }, replaceChildren(...items) { this.children = items; }}; }
const context = {element};
vm.runInNewContext(source.slice(start, end), context);
const panel = element('div');
context.renderAttemptAnswers(panel, {available:true, unreadable:1, truncated:false, items:[
  {file:'answers/0000.json', data:{type:'fields_inspected',fields:[{question:'Work authorization',required:true,sensitive:true}],omitted:2}},
  {file:'answers/0001.json', data:{type:'fields_requested', fields:[{question:'Name',value:{text:'Fictional Candidate'}}]}},
  {file:'answers/0002.json', data:{type:'fill_incomplete',answer_file:'answers/0001.json',completed_commands:[{label:'Name'}]}}
]});
const text = panel.children.flatMap(item => [item.text, ...item.children.map(child => child.text)]).join('\n');
assert.match(text, /Fictional Candidate/);
assert.match(text, /Name: Fictional Candidate/);
assert.doesNotMatch(text, /\{"text"/);
assert.match(text, /Fill interrupted/);
assert.match(text, /current values are unconfirmed/);
assert.match(text, /1 unreadable/);
context.renderAttemptAnswers(panel, {available:false,items:[]});
assert.ok(panel.children.some(item => item.text.includes('older attempt')));
console.log('Answer history UI checks passed.');

assert.match(text, /Work authorization · required · sensitive or review required/);
assert.match(text, /2 questions were omitted/);
