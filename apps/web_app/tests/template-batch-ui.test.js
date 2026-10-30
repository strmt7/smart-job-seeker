const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf('function batchTemplateSuggestions(');
const end = source.indexOf('function showDocument(', start);
assert.ok(start >= 0 && end > start);
function node(tag, text = '') {
  return {tag, textContent:text, disabled:false, children:[], handlers:{},
    append(...items){this.children.push(...items);}, replaceChildren(...items){this.children=[...items];},
    setAttribute(){}, addEventListener(name,handler){this.handlers[name]=handler;},
    querySelectorAll(tag){return this.children.flatMap(child=>[...(child.tag===tag?[child]:[]),...(child.querySelectorAll?.(tag)||[])]);}};
}
async function fixture(failSecond = false) {
  const calls=[], notices=[], changes=[], text={value:'Built dashboards\nLed research',selectionStart:0,selectionEnd:29}, jobSelect={value:'job'}, busy=[];
  const context={element:(tag,text)=>node(tag,text), TextEncoder, setTimeout:f=>f(), notify:message=>notices.push(message),
    api:async(path,body)=>{calls.push({path,body});if(failSecond && path.endsWith('-start') && body.before==='Led research')throw Error('Model unavailable');return path.endsWith('-start')?{task_id:body.before}:{status:'complete',document_id:'doc',job_id:'job',before:body.task_id,digest:'a'.repeat(64),suggestion:{text:`Revised ${body.task_id}`,sources:['Fictional source']}};}};
  const create=vm.runInNewContext(`(${source.slice(start,end)})`,context);
  const section=create({text,documentResult:{id:'doc'},jobSelect,changes,renderQueue:()=>{},isBusy:()=>false,setBusy:value=>busy.push(value)});
  await section.children[0].handlers.click();
  return {section,calls,notices,changes,jobSelect,busy};
}
(async()=>{
  let f=await fixture();
  assert.equal(f.calls.length,4);
  assert.deepEqual(f.busy,[true,false]);
  assert.match(f.section.children[1].textContent,/2 suggestions ready/);
  const buttons=f.section.children[2].querySelectorAll('button');
  assert.equal(buttons.length,2);
  buttons[0].handlers.click();buttons[1].handlers.click();
  assert.equal(f.changes.length,2);
  assert.equal(f.changes[0].suggestion_digest,'a'.repeat(64));
  assert.equal(f.changes[0].suggestion_job_id,'job');
  f=await fixture(true);
  assert.match(f.section.children[1].textContent,/Stopped after 1 of 2/);
  assert.equal(f.section.children[2].querySelectorAll('button').length,1);
  f.jobSelect.value='other';
  f.section.children[2].querySelectorAll('button')[0].handlers.click();
  assert.equal(f.changes.length,0);
  assert.match(f.notices[0],/job or queue changed/);
  console.log('Template batch suggestion UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
