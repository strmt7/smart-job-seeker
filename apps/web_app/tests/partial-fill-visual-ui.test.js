const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf(' async function reviewAfterFillFailure(');
const end = source.indexOf('\n visualButton.addEventListener', start);
assert.ok(start >= 0 && end > start);
assert.ok(source.includes('catch(e){await reviewAfterFillFailure(e,fill);}'), 'the form fill handler must route failures to visual review');

async function exercise(message, visionFails=false){
  const events = [], fill = {disabled:true};
  const context = {
    fillCompleted:false, fillInProgress:true, visualButton:{disabled:true},
    notify:message=>events.push(['notify',message]),
    panel:{append:item=>events.push(['warning',item.text])},
    visualPanel:{replaceChildren:item=>events.push(['message',item.text]),append:item=>events.push(['visual-error',item.text])},
    element:(_tag,text)=>({text}),
    inspectVisual:async()=>{events.push(['inspect',context.fillInProgress,context.visualButton.disabled]);if(visionFails)throw new Error('fixture unavailable');},
    refresh:async()=>events.push(['refresh']),
  };
  const handle = vm.runInNewContext(`${source.slice(start,end)}; reviewAfterFillFailure`,context);
  await handle({message},fill);
  return {context,events,fill};
}
(async()=>{
  const partial = await exercise('Autofill stopped after 1 successful field commands; review the browser.');
  assert.ok(partial.events.some(event=>event[0]==='warning'));
  assert.ok(partial.events.some(event=>event[0]==='inspect' && event[1]===false && event[2]===false));
  assert.equal(partial.context.fillCompleted,true);
  assert.equal(partial.fill.disabled,true);
  const recordFailure = await exercise('No fields were filled because the attempt record could not be written: fixture');
  assert.ok(recordFailure.events.every(event=>event[0]!=='inspect'));
  assert.equal(recordFailure.fill.disabled,false);
  assert.equal(recordFailure.context.fillCompleted,false);
  const uncertain = await exercise('Connection interrupted');
  assert.ok(uncertain.events.some(event=>event[0]==='inspect'));
  const failedVision = await exercise('Connection interrupted',true);
  assert.ok(failedVision.events.some(event=>event[0]==='visual-error' && event[1].includes('Inspect the live form manually')));
  assert.equal(failedVision.fill.disabled,true);
  console.log('Partial-fill visual review UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
