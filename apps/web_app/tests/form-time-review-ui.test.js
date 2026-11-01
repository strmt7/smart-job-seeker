const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const render = source.split('\n').find(line => line.includes('const selections=[],manualAnswers=[]'));
const payload = source.split('\n').find(line => line.startsWith('function formAnswerPayload('));
assert.ok(render && payload);

function node(tag, text = '') {
  return {tag, textContent: text, children: [], value: '', attributes: {},
    append(...items) { this.children.push(...items); },
    setAttribute(name, value) { this.attributes[name] = value; },
    addEventListener() {}};
}
const panel = node('div');
const context = {
  panel, result: {fields: [{selector: '#time', label: 'Interview time', field_type: 'time', required: true, sensitive: false}], suggestions: []},
  document: {createElement: tag => node(tag)},
  element: (tag, text) => node(tag, text),
  formPatternNotice: () => '', formEditorNotice: () => '', attachFormLengthCounter: () => {},
};
vm.createContext(context);
vm.runInContext(`${payload}\n${render}\nglobalThis.testAnswers=manualAnswers;`, context);
assert.equal(context.testAnswers.length, 1);
const answer = context.testAnswers[0];
assert.equal(answer.input.type, 'time');
assert.equal(answer.input.attributes['aria-label'], 'Interview time');
answer.input.value = '13:30';
assert.equal(JSON.stringify(context.formAnswerPayload(answer)), JSON.stringify({selector: '#time', value: '13:30'}));
for (const [kind, value] of [['date', '2026-11-01'], ['month', '2026-11'], ['time', '13:30'], ['date_time_local', '2026-11-01T13:30']]) {
  const mapped = {
    panel: node('div'),
    result: {fields: [{selector: '#field', label: 'Available', field_type: kind, sensitive: false}], suggestions: [{field_selector: '#field', value: {date: value}}]},
    document: {createElement: tag => node(tag), createTextNode: text => node('#text', text)},
    element: (tag, text) => node(tag, text), formPatternNotice: () => '', formEditorNotice: () => '',
    attachFormLengthCounter: () => {}, preselectFormSuggestion: () => true,
  };
  vm.createContext(mapped);
  vm.runInContext(`${render}\nglobalThis.testSelections=selections;`, mapped);
  assert.equal(mapped.testSelections.length, 1);
  assert.equal(mapped.testSelections[0].input.type, kind==='date_time_local'?'datetime-local':kind);
  assert.equal(mapped.testSelections[0].input.value, value);
  assert.equal(mapped.testSelections[0].original, value);
}
const local={panel:node('div'),result:{fields:[{selector:'#dt',label:'Appointment',field_type:'date_time_local',required:true,sensitive:false}],suggestions:[]},document:{createElement:tag=>node(tag)},element:(tag,text)=>node(tag,text),formPatternNotice:()=>'',formEditorNotice:()=>'',attachFormLengthCounter:()=>{}};vm.createContext(local);vm.runInContext(`${payload}\n${render}\nglobalThis.testAnswers=manualAnswers;`,local);assert.equal(local.testAnswers[0].input.type,'datetime-local');local.testAnswers[0].input.value='2026-11-01T13:30';assert.equal(JSON.stringify(local.formAnswerPayload(local.testAnswers[0])),JSON.stringify({selector:'#dt',value:'2026-11-01T13:30'}));
console.log('Reviewed date and time input UI checks passed.');
