const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js','utf8');
const start = source.indexOf('if(a.preference_audit &&');
const end = source.indexOf('if(a.requirement_audit)',start);
assert.ok(start >= 0 && end > start);
const render = vm.runInNewContext(`(function(a,output,element){${source.slice(start,end)}})`);
const element = (_tag,text)=>({text});
function textFor(a){const output={nodes:[],append(node){this.nodes.push(node)}};render(a,output,element);return output.nodes.map(node=>node.text).join(' ');}
assert.match(textFor({preference_audit:{status:'completed',added:2,audit_limit_reached:true}}), /2 possible preferences added.*three-preference limit/);
assert.match(textFor({preference_audit:{status:'unavailable',added:0}}), /unavailable; no preferences added/);
assert.equal(textFor({preference_audit:{status:'no_instructions'}}),'');
console.log('Preference audit UI checks passed.');
