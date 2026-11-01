const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf('if(a.source_coverage){');
const end = source.indexOf("if(['criteria-v1','criteria-v2']", start);
assert.ok(start >= 0 && end > start);
const render = vm.runInNewContext('(function(a,output,element){' + source.slice(start,end) + '})');
function messages(coverage) { const output={nodes:[],append(node){this.nodes.push(node)}}; render({source_coverage:coverage},output,(_tag,text)=>({text})); return output.nodes.map(node=>node.text).join(' '); }
assert.match(messages({reviewed_facts_total:503,reviewed_facts_bounded:500,reviewed_facts_oversized:1,reviewed_facts_over_limit:2}), /500 of 503.*1 exceeded.*2 exceeded.*not assessed/);
assert.equal(messages({reviewed_facts_total:2,reviewed_facts_bounded:2,reviewed_facts_oversized:0,reviewed_facts_over_limit:0}), '');
assert.match(messages({}), /coverage is unavailable.*reassess/);
assert.match(messages({description_present:false}), /saved job has no description/);
console.log('Fit source coverage UI checks passed.');
