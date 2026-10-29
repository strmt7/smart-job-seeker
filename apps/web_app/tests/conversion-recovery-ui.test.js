const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const line = source.split('\n').find(item => item.startsWith('async function recoverPendingConversion('));
assert.ok(line, 'conversion recovery helper is present');

(async () => {
  let stored = 'older-task', followed = [], calls = 0, response = {active:true,task_id:'new-task'};
  const sessionStorage = {getItem:()=>stored,setItem:(_,value)=>{stored=value;}};
  const context = {conversionFollowing:false,sessionStorage,api:async(path)=>{calls++;assert.equal(path,'/api/documents/convert-pdf-pending');return response;},updateConversionControl:()=>{},followConversion:async id=>followed.push(id)};
  const recover = vm.runInNewContext(`(${line})`, context);
  await recover();
  assert.equal(stored, 'new-task');
  assert.deepEqual(followed, ['new-task']);
  response = {active:false};
  followed = [];
  await recover();
  assert.deepEqual(followed, ['new-task']);
  stored = null;
  followed = [];
  await recover();
  assert.deepEqual(followed, []);
  context.conversionFollowing = true;
  await recover();
  assert.equal(calls, 3);
  assert.ok(source.includes('void recoverPendingConversion().catch(conversionError)'));
  console.log('Conversion recovery UI checks passed.');
})().catch(error => { console.error(error); process.exitCode = 1; });
