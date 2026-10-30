const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const helper = source.split('\n').find(line => line.startsWith('function applicationCheckState('));
const messageHelper = source.split('\n').find(line => line.startsWith('function applicationCheckMessage('));
const start = source.indexOf('async function runCandidateBatch()');
const end = source.indexOf('\ndocument.querySelector(\'#candidate-batch-start\')', start);
assert.ok(helper && messageHelper && start >= 0 && end > start);
const message = vm.runInNewContext(`(${messageHelper})`);

async function run(current) {
  const checks = new Map(), nodes = new Map();
  const node = () => ({textContent:'',replaceChildren(){},append(){}});
  const context = {
    candidateBatch: {items:[{url:'https://example.org/role',title:'Fictional role'}],index:0,phase:'check',task:'task',job:'job',events:[],stop:false,verify:true},
    candidateBatchRunning:false, applicationChecks:checks, selectedCandidates:new Map(),
    renderCandidateBatch(){}, saveCandidateBatch(){}, updateCandidateSelection(){}, refresh:async()=>{},
    setTimeout:callback=>callback(), element:()=>node(),
    document:{querySelector:selector=>{if(!nodes.has(selector))nodes.set(selector,node());return nodes.get(selector);}},
    api:async()=>({status:'complete',checked_url:'https://example.org/role',result:'Manual review needed',current}),
  };
  const execute = vm.runInNewContext(`${helper}\n${source.slice(start,end)}\nrunCandidateBatch`, context);
  await execute();
  return {check:checks.get('job'),status:nodes.get('#candidate-batch-status').textContent};
}
(async()=>{
  for (const current of [true,false]) {
    const {check,status} = await run(current);
    assert.equal(check.url,'https://example.org/role');
    assert.equal(check.text,'Manual review needed');
    assert.equal(check.current,current);
    assert.equal(message(check,{canonical_url:'https://example.org/role'}),current?'Manual review needed':'The application check is out of date; inspect the current listing again.');
    assert.equal(message(check,{canonical_url:'https://example.org/new-role'}),'The application check is out of date; inspect the current listing again.');
    assert.match(status,/Finished: 1 of 1/);
  }
  console.log('Batch application-check UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
