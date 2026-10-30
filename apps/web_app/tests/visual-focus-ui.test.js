const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf(" fieldFocusButton.addEventListener('click'");
const end = source.indexOf('\n let fillButton=', start);
assert.ok(start >= 0 && end > start);
async function run(fail, retained=true, otherFailed=false) {
  let handler; const calls = [], messages = [];
  const context = {
    fieldFocusButton: {disabled:false, addEventListener:(_, fn)=>{handler=fn;}},
    fieldFocusSelect: {value:'fictional-selector'}, fillInProgress:false,
    visualButton: {disabled:false}, result: {session_id:'fictional-session'},
    image: {isConnected:true, src:''}, panel: {append:()=>{}},
    visualPanel: {replaceChildren:x=>messages.push(x.text), append:x=>messages.push(x.text)},
    element:(_tag,text)=>({text}), refresh:async()=>{},
    api: async (path, body)=>{calls.push([path, body]);if(fail)throw new Error('fixture changed');return {screenshot:'fictional',record_saved:true,value_retained:retained,verified_count:1,unverified_count:otherFailed?1:0,failed_fields:otherFailed?['Far email']:[]};},
  };
  vm.runInNewContext(source.slice(start,end),context);
  await handler();
  assert.equal(context.fieldFocusButton.disabled,false);
  assert.equal(context.visualButton.disabled,false);
  assert.equal(calls[0][0],'/api/form/visual-focus');
  assert.equal(calls[0][1].selector,'fictional-selector');
  if(fail) assert.ok(messages[0].includes('fixture changed'));
  else { assert.equal(context.image.src,'data:image/png;base64,fictional'); assert.ok(messages[0].includes('earlier visual results are no longer current')); if(!retained) assert.ok(messages[0].includes('could not be verified')); if(otherFailed) assert.ok(messages.some(message=>message.includes('Far email')) && messages.some(message=>message.includes('1 not checked'))); }
}
(async()=>{await run(false);await run(false,false);await run(false,true,true);await run(true);console.log('Visual focus UI checks passed.');})().catch(error=>{console.error(error);process.exitCode=1;});
