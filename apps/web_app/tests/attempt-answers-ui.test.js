const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf('function renderAttemptAnswers(');
const end = source.indexOf('\nfunction ', start);
assert.ok(start >= 0 && end > start);
function element(tag, text = '') { return {tag, text, children: [], append(...items) { this.children.push(...items); }, replaceChildren(...items) { this.children = items; }, addEventListener(event,fn) { this[event]=fn; }}; }
const context = {element};
vm.runInNewContext(source.slice(start, end), context);
const panel = element('div');
context.renderAttemptAnswers(panel, {available:true, unreadable:1, truncated:false, items:[
  {file:'answers/0000.json', data:{type:'fields_inspected',origin:'https://example.invalid',fields:[{question:'Work authorization',required:true,sensitive:true,field_type:{select:{options:['Yes','No']}}}],omitted:2}},
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
assert.match(text, /Page origin: https:\/\/example.invalid/);
assert.match(text, /Available choices: Yes · No/);

(async()=>{const calls=[];context.api=async(path,body)=>{calls.push(body.after);return {available:true,items:[{data:{type:'fields_inspected',fields:[{question:`Question ${body.after}`}]}}],next_after:body.after===0?64:null}};await context.loadAttemptAnswers(panel,'job','attempt');assert.match(panel.children[1].children[2].text,/Question 0/);await panel.children.find(item=>item.text==='Next history page').click();assert.match(panel.children[1].children[2].text,/Question 64/);await panel.children.find(item=>item.text==='First history page').click();assert.deepEqual(calls,[0,64,0]);console.log('Answer history paging checks passed.');})().catch(error=>{console.error(error);process.exitCode=1});
