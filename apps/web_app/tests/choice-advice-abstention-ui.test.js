const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const line=source.split('\n').find(value=>value.startsWith('function choiceAdviceAbstentionMessage('));assert.ok(line);
const message=vm.runInNewContext(`(${line})`);
assert.match(message('model_abstained'),/did not select/);
assert.match(message('verification_inconclusive'),/second model check/);
assert.match(message(undefined),/No option/);
assert.equal((source.match(/choiceAdviceAbstentionMessage\(response\.advice\?\.abstention_reason\)/g)||[]).length,2);
console.log('Choice advice abstention messages passed.');
