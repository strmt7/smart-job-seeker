const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const line=source.split('\n').find(value=>value.startsWith('function priorDuplicateDecisionText('));assert.ok(line);
const describe=vm.runInNewContext(`(${line})`);
assert.match(describe({status:'pending',previous_decision:{status:'different',changed_job:'a'}},'a'),/Previously marked as different.*This listing changed/);
assert.match(describe({status:'pending',previous_decision:{status:'same',preferred_job:'b',changed_job:'b'}},'a'),/other listing was kept.*other listing changed/);
assert.equal(describe({status:'same',previous_decision:{status:'same'}},'a'),null);
assert.equal(describe({status:'pending'},'a'),null);
console.log('Duplicate review context checks passed.');

assert.match(source,/keep_or_first:job.id,other:otherId,same,revision:pair.revision/);
