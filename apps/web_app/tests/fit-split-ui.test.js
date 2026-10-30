const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const getFunction = name => {
  const line = source.split('\n').find(value => value.startsWith(`function ${name}(`));
  assert.ok(line, `${name} must exist`);
  return line;
};
function fakeNode(tag, text = '') {
  return {tag, textContent:text, children:[], listeners:{}, append(...nodes){this.children.push(...nodes);}, setAttribute(){}, addEventListener(event, callback){const previous=this.listeners[event];this.listeners[event]=()=>{if(previous)previous();callback();};}};
}
const context = {element:(tag,text)=>fakeNode(tag,text), document:{createElement:tag=>fakeNode(tag)}};
for (const name of ['fitRubricScore', 'fitSplitItems', 'fitSplitControls', 'fitReviewControls']) {
  context[name] = vm.runInNewContext(`(${getFunction(name)})`, context);
}
const item = {source:'listing',excerpt:'C and C++ programming',importance:'required',status:'supported'};
assert.equal(context.fitSplitItems(item,'C','C++').length, 2);
assert.equal(context.fitSplitItems({...item,excerpt:'C++'},'C','C++'), null);
assert.equal(context.fitSplitItems(item,'C','Rust'), null);
assert.equal(context.fitSplitItems(item,'C','C'), null);
const controls = context.fitReviewControls([item]);
const split = controls.children.find(node=>node.tag==='details');
split.open = true;
split.listeners.toggle();
assert.match(controls.children.at(-1).textContent, /Enter two separate exact excerpts/);
const inputs = split.children.filter(node=>node.tag==='label' && node.children[0].tag==='input').map(node=>node.children[0]);
const statuses = split.children.filter(node=>node.tag==='label' && node.children[0].tag==='select').map(node=>node.children[0]);
inputs[0].value='C'; inputs[1].value='C++'; inputs[1].listeners.input();
assert.equal(controls.children.at(-1).textContent, 'Temporary estimate: 50% · 0–100% range');
statuses[0].value='supported'; statuses[0].listeners.change();
assert.equal(controls.children.at(-1).textContent, 'Temporary estimate: 75% · 50–100% range');
split.open=false; split.listeners.toggle();
assert.equal(controls.children.at(-1).textContent, 'Temporary estimate: 100% · 100–100% range');
console.log('Fit split UI checks passed.');
