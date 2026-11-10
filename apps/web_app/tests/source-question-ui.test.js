'use strict';
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
class Node {
  constructor(tag,textContent='',className=''){Object.assign(this,{tag,textContent,className,children:[],listeners:{},attributes:{}});}
  append(...nodes){this.children.push(...nodes);}
  replaceChildren(...nodes){this.children=nodes;}
  setAttribute(key,value){this.attributes[key]=value;}
  addEventListener(type,handler){this.listeners[type]=handler;}
}
const all=node=>[node,...node.children.flatMap(all)],button=(node,text)=>all(node).find(n=>n.tag==='button'&&n.textContent===text);
function harness(api){
  const window={};vm.runInNewContext(fs.readFileSync('apps/web_app/web/source-copy.js','utf8'),{window,setTimeout:resolve=>resolve(),Date});
  return window.WaypointSourceCopies.outlineView({status:'observed',controls:[{number:1,role:'textbox',label:'Describe relevant experience',context:[],disabled:null},{number:2,role:'button',label:'Continue'},{number:3,role:'textbox',label:'Incomplete question',context_incomplete:true}]},(...args)=>new Node(...args),{api,jobId:'job-a',attemptId:'copy-a',renderEvidence:(container,answer)=>container.append(new Node('p',`Shared evidence renderer: ${answer?.evidence_check?.status||'unavailable'}`))});
}
(async()=>{
  const calls=[];let polls=0;
  const root=harness(async(route,body)=>{
    calls.push({route,body});assert.equal(body.id,'job-a');assert.equal(body.attempt_id,'copy-a');assert.equal(body.number,1);
    if(route.endsWith('/question'))return {question:'Engineering\nDescribe relevant experience',digest:'bound',notice:'Review the current question.',drafts:[{current:false,answer:{text:'Historical fictional answer'}}]};
    if(route.endsWith('/answer-start')){assert.equal(body.digest,'bound');return {task_id:'draft-a'};}
    assert(route.endsWith('/answer-status'));assert.equal(body.task_id,'draft-a');
    return ++polls===1?{status:'running'}:{status:'complete',saved_file:'question-draft-draft-a.json',answer:{text:'I built a fictional service.',sources:['Built a fictional service.'],evidence_check:{status:'supported'}}};
  });
  assert.equal(all(root).filter(n=>n.textContent==='Prepare answer').length,1);
  assert.equal(calls.length,0,'Opening a page outline must not start a model');
  await button(root,'Prepare answer').listeners.click();assert.equal(calls.length,1);
  assert(all(root).some(n=>n.textContent==='Engineering\nDescribe relevant experience'));
  assert(all(root).some(n=>/Historical draft/.test(n.textContent)));
  const draft=button(root,'Draft answer');assert.equal(draft.type,'button');
  await draft.listeners.click();
  assert.equal(polls,2);assert.equal(draft.disabled,false);
  assert(all(root).some(n=>n.tag==='textarea'&&n.value==='I built a fictional service.'&&n.readOnly===true));
  assert(all(root).some(n=>n.textContent==='Shared evidence renderer: supported'));
  assert(all(root).some(n=>/question-draft-draft-a.json/.test(n.textContent)));
  assert(!calls.some(c=>/fill|submit|inspect/.test(c.route)));
  const stale=harness(async route=>route.endsWith('/question')?{question:'Question',digest:'old',drafts:[]}:route.endsWith('/answer-start')?{task_id:'stale'}:{status:'failed',error:'Supporting information changed'});
  await button(stale,'Prepare answer').listeners.click();await button(stale,'Draft answer').listeners.click();
  assert(all(stale).some(n=>n.textContent==='Supporting information changed'));
  assert(!all(stale).some(n=>n.tag==='textarea'));
  console.log('Saved-question review, deliberate drafting, bound polling, history, readonly output and stale-result refusal passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
