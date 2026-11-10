const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const review = vm.runInNewContext(`(${source.split('\n').find(line=>line.startsWith('function showTemplateSuggestionReview('))})`,{element:(tag,text)=>node(tag,text),showAnswerEvidence:()=>{}});
for (const [original, replacement, expected] of [
  ['I’ve built tools.', "I've built tools.", true],
  ['Built  tools.', 'Built tools.', true],
  ['Built C++ tools.', 'Built C tools.', false],
  ['I built tools.', 'I did not build tools.', false],
]) {
  const box=node('div'); review(box,{text:replacement},original);
  assert.equal(box.children.some(item=>item.textContent.includes('Only spacing or apostrophe style changed')),expected);
}
for (const [status, message] of [['revised','second drafting attempt produced'],['failed','did not produce an accepted alternative'],['needs_review','did not produce an accepted alternative'],['unchanged','did not produce an accepted alternative']]) {
  const box=node('div'); review(box,{text:'Revised passage',cosmetic_retry:status},'Original passage');
  assert.ok(box.children.some(item=>item.textContent.includes(message)));
}
const start = source.indexOf('function batchTemplateSuggestions(');
const end = source.indexOf('\nfunction ', start);
assert.ok(start >= 0 && end > start);
function node(tag, text = '') {
  return {tag, textContent:text, disabled:false, children:[], handlers:{},
    append(...items){this.children.push(...items);}, replaceChildren(...items){this.children=[...items];},
    setAttribute(){}, addEventListener(name,handler){this.handlers[name]=handler;},
    querySelectorAll(tag){return this.children.flatMap(child=>[...(child.tag===tag?[child]:[]),...(child.querySelectorAll?.(tag)||[])]);}};
}
function flatten(node){return [node,...(node.children||[]).flatMap(flatten)];}
async function fixture(failSecond = false, automatic = false, emptyPlan = false) {
  const calls=[], notices=[], changes=[], text={value:'Built dashboards\nLed research',selectionStart:0,selectionEnd:29}, jobSelect={value:'job'}, busy=[];
  const context={element:(tag,text)=>node(tag,text), TextEncoder, setTimeout:f=>f(), notify:message=>notices.push(message),
    selectedFactsDetails:()=>null, factCrossCheckMessage:()=>'',
    api:async(path,body)=>{calls.push({path,body});if(path==='/api/documents/template-plan-start')return {task_id:'plan'};if(path==='/api/documents/template-plan-status')return {status:'complete',document_id:'doc',job_id:'job',digest:'b'.repeat(64),plan:{passages:emptyPlan?[]:[{before:'Built dashboards',reason:'Clarify relevant work'},{before:'Led research',reason:'Highlight research'}],coverage:{nonempty_lines:2,unique_lines:2,presented:2,repeated:0,unsupported:0,omitted:0},rejected:[]}};if(failSecond && path.endsWith('-start') && body.before==='Led research')throw Error('Model unavailable');return path.endsWith('-start')?{task_id:body.before}:{status:'complete',document_id:'doc',job_id:'job',before:body.task_id,digest:'a'.repeat(64),suggestion:{text:body.task_id==='Led research'?body.task_id:`Revised ${body.task_id}`,sources:['Fictional source'],first_person_opening_changed:body.task_id==='Led research',additional_sources_cited:body.task_id==='Led research',evidence_check:body.task_id==='Led research'?{status:'invalid'}:{status:'completed',claims:[{status:'unsupported',excerpt:'Revised',explanation:'Not established'}]}}};}};
  context.showAnswerEvidence=vm.runInNewContext(`(${source.split('\n').find(line=>line.startsWith('function showAnswerEvidence('))})`,context);
  context.showTemplateSuggestionReview=vm.runInNewContext(`(${source.split('\n').find(line=>line.startsWith('function showTemplateSuggestionReview('))})`,context);
  context.templateSuggestionDetails=vm.runInNewContext(`(${source.split('\n').find(line=>line.startsWith('function templateSuggestionDetails('))})`,context);
  context.followTemplateSuggestion=(task,endpoint='/api/documents/template-suggest-status')=>context.api(endpoint,{task_id:task});
  const create=vm.runInNewContext(`(${source.slice(start,end)})`,context);
  const section=create({text,documentResult:{id:'doc'},jobSelect,changes,renderQueue:()=>{},isBusy:()=>false,setBusy:value=>busy.push(value)});
  await section.children[0].children[automatic?0:1].handlers.click();
  return {section,calls,notices,changes,jobSelect,busy};
}
(async()=>{
  let f=await fixture();
  assert.equal(f.calls.length,4);
  assert.deepEqual(f.busy,[true,false]);
  assert.match(f.section.children[1].textContent,/2 suggestions ready/);
  const cards=f.section.children[2].children;
  assert.ok(flatten(cards[0]).some(item=>/unsupported: Revised/.test(item.textContent)));
  assert.ok(flatten(cards[0]).some(item=>/has not been independently verified/.test(item.textContent)));
  assert.ok(flatten(cards[1]).some(item=>/original passage unchanged/.test(item.textContent)));
  assert.ok(flatten(cards[1]).some(item=>/changed whether the passage begins/.test(item.textContent)));
  assert.ok(flatten(cards[1]).some(item=>/qualifications beyond the selected passage/.test(item.textContent)));
  assert.ok(flatten(cards[1]).some(item=>/unavailable or incomplete/.test(item.textContent)));
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
  assert.ok(source.includes('templateSuggestionDetails(suggestion,proposed,selected)'));
  f=await fixture(false,true);
  assert.equal(f.calls.length,6);assert.equal(f.calls[0].path,'/api/documents/template-plan-start');
  assert.equal(f.section.children[2].querySelectorAll('button').length,2);assert.equal(f.changes.length,0);
  assert.match(f.section.children[2].children[0].textContent,/2 of 2 unique passages considered/);
  assert.deepEqual(f.busy,[true,false]);
  f=await fixture(false,true,true);assert.equal(f.calls.length,2);
  assert.match(f.section.children[1].textContent,/does not establish/);assert.equal(f.changes.length,0);
  console.log('Template batch suggestion UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
