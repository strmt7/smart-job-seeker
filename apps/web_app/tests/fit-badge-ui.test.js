const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const line = source.split('\n').find(value => value.startsWith('function fitBadgeLabel('));
assert.ok(line, 'fit badge formatter must exist');
const label = vm.runInNewContext(`(${line})`);
assert.equal(label(55, {lower: 10, upper: 100}), '55% AI estimate · 10–100% range');
assert.equal(label(75, {lower: 75, upper: 75}), '75% AI estimate · 75–75% range');
assert.equal(label(55, null), '55% AI estimate · range unavailable');
assert.equal(label(55, {lower: 60, upper: 100}), '55% AI estimate · range unavailable');
assert.equal(label(-1, {lower: 0, upper: 100}), 'Estimate unavailable');
assert.match(label(75,{lower:50,upper:100},'needs_review'), /check preferences/);
assert.match(label(75,{lower:50,upper:100},'possible_conflict'), /possible preference conflict/);
assert.doesNotMatch(label(75,{lower:50,upper:100},'included'), /check preferences|conflict/);
assert.ok(source.includes('fitBadgeLabel(item.score,item.score_range,item.preference_coverage)'), 'batch badges must use their stored range');
assert.ok(source.includes('fitBadgeLabel(score,result.assessment?.score_range,result.assessment?.preference_coverage)'), 'new assessments must use their range');
const scoreLine = source.split('\n').find(value => value.startsWith('function fitRubricScore('));
assert.ok(scoreLine, 'temporary correction calculation must exist');
const calculate = vm.runInNewContext(`(${scoreLine})`);
const rubric = [
  {importance:'required',status:'supported'},
  {importance:'required',status:'unknown'},
  {importance:'preferred',status:'contradicted'}
];
assert.equal(JSON.stringify(calculate(rubric)), JSON.stringify({score:64,lower:43,upper:86}));
rubric[0].status='unknown';
assert.equal(JSON.stringify(calculate(rubric)), JSON.stringify({score:43,lower:0,upper:86}));
assert.equal(calculate([{importance:'required',status:'invented'}]), null);
assert.ok(source.includes('it does not save corrections or change the job badge'), 'temporary edits must be clearly labelled');
assert.ok(source.includes('fitReviewControls(a.rubric)'), 'assessment must expose the controls');
const controlsLine = source.split('\n').find(value => value.startsWith('function fitReviewControls('));
function fakeNode(tag,text='') { return {tag,textContent:text,children:[],listeners:{},append(...nodes){this.children.push(...nodes);},setAttribute(){},addEventListener(event,callback){this.listeners[event]=callback;}}; }
const context = {fitRubricScore:calculate,element:(tag,text)=>fakeNode(tag,text),document:{createElement:tag=>fakeNode(tag)}};
const controls = vm.runInNewContext(`(${controlsLine})`,context)(rubric);
const selects = controls.children.filter(node=>node.tag==='label').map(node=>node.children[0]);
assert.equal(selects.length,3);
assert.equal(controls.children.at(-1).textContent,'Temporary estimate: 43% · 0–86% range');
selects[1].value='supported';
selects[1].listeners.change();
assert.equal(controls.children.at(-1).textContent,'Temporary estimate: 64% · 43–86% range');
selects[0].value='exclude';
selects[0].listeners.change();
assert.equal(controls.children.at(-1).textContent,'Temporary estimate: 75% · 75–75% range');
selects[1].value='exclude';
selects[2].value='exclude';
selects[2].listeners.change();
assert.equal(controls.children.at(-1).textContent,'Temporary estimate unavailable (select 1–12 criteria).');
console.log('Fit badge UI checks passed.');
