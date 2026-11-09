const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
function element(tag,text=''){return {tagName:tag.toUpperCase(),textContent:text,value:'',children:[],listeners:{},isConnected:true,append(...items){this.children.push(...items);},replaceChildren(...items){this.children=items;},setAttribute(key,value){(this.attributes??={})[key]=value;},addEventListener(key,handler){this.listeners[key]=handler;},dispatchEvent(event){this.listeners[event.type]?.(event);},get options(){return this.children;}};}
const walk=root=>[root,...root.children.flatMap(item=>item?.children?walk(item):[])];
const find=(root,text)=>walk(root).find(el=>el.textContent===text);
function fixture({empty=false}={}){
 const calls=[],expiries=[],notices=[],inputs=[element('input'),element('input'),element('input')];inputs[1].value='Keep manual answer';
 let release,step=0;
 const response={status:'complete',expires_in_seconds:580,advice:{mode:'form',cancelled:true,proposals:[{selector:'#first',label:'Current company',entry_label:'Engineer at Fictional Current',value:{text:'Fictional Current'},entry_id:'a'.repeat(32),component:'employer',source_sha256:'c'.repeat(64)},{selector:'#third',label:'Previous title',entry_label:'Lead at Fictional Prior',value:{text:'Lead'},entry_id:'b'.repeat(32),component:'title',source_sha256:'d'.repeat(64)}],issues:[{error:'Local inference paused: fictional memory limit <script>'},{error:'Local inference paused: fictional memory limit <script>'}],manual_review:[{label:'Referee',reason:'wrong_subject'}]}};
 if(empty)response.advice.proposals=[];
 const context={document:{createElement:element,createTextNode:text=>({textContent:text})},Option:function(text,value){return {text,value};},Event:class{constructor(type){this.type=type;}},TextEncoder,setTimeout:fn=>{fn();return 0;},CandidateHistory:{parse:()=>({employment:[{id:'a'.repeat(32),employer:'Fictional Current',title:'Engineer',reviewed:true},{id:'b'.repeat(32),employer:'Fictional Prior',title:'Lead',reviewed:true}],education:[]})},notify:message=>notices.push(message),api:async(path,body)=>{calls.push({path,body});if(path.endsWith('-start'))return {task_id:'owned-task',expires_in_seconds:600};if(path.endsWith('-cancel')){release();return {cancel_requested:true};}if(++step===1)return {status:'running',expires_in_seconds:590,progress:{processed_fields:1,total_fields:2,proposed_fields:1}};if(step===2)await new Promise(resolve=>{release=resolve;});return response;}};
 vm.createContext(context);vm.runInContext(fs.readFileSync('apps/web_app/web/app.js','utf8').split('\n').filter(line=>line.startsWith('function clearFormHistorySource(')||line.startsWith('function linkFormHistorySource(')).join('\n'),context);vm.runInContext(fs.readFileSync('apps/web_app/web/history-mapping.js','utf8'),context);
 const selectors=['#first','#second','#third'];
 const box=context.HistoryMapping.control({session_id:'owned-session',fields:selectors.map((selector,i)=>({selector,label:['Current company','Existing answer','Previous title'][i],field_type:'text',sensitive:false}))},selectors.map((selector,i)=>({selector,input:inputs[i],kind:'text'})),{},{onExpiry:seconds=>expiries.push(seconds)});
 return {box,inputs,calls,expiries,notices};
}
(async()=>{
 const x=fixture(),all=find(x.box,'Suggest history answers for blank fields');const running=all.listeners.click();
 for(let i=0;i<20&&x.calls.length<3;i++)await Promise.resolve();
 assert.equal(x.calls[0].body.entry_id,'all');assert.deepEqual(Array.from(x.calls[0].body.selectors),['#first','#third']);
 assert.ok(walk(x.box).some(el=>el.textContent==='1 of 2 questions processed · 1 suggestions'));
 assert.equal(x.inputs[1].value,'Keep manual answer');assert.equal(all.disabled,true);
 const cancel=find(x.box,'Stop preparing answers');assert.equal(cancel.hidden,false);await cancel.listeners.click();await running;
 assert.deepEqual(JSON.parse(JSON.stringify(x.calls.find(call=>call.path.endsWith('-cancel')).body)),{task_id:'owned-task',session_id:'owned-session'});
 assert.equal(cancel.hidden,true);assert.equal(all.disabled,false);assert.ok(x.expiries.includes(600)&&x.expiries.includes(590)&&x.expiries.includes(580));
 const proposals=walk(x.box).filter(el=>el.className==='history-proposal');assert.equal(proposals.length,2);assert.ok(proposals.every(row=>row.children[0].checked===false));assert.equal(x.inputs[0].value,'');assert.equal(x.inputs[2].value,'');
 assert.equal(walk(x.box).filter(el=>el.className==='history-proposal-source').length,2);
 assert.ok(find(x.box,'Why some answers need review'));assert.equal(walk(x.box).filter(el=>el.textContent==='Local inference paused: fictional memory limit <script>').length,1);
 const selectAll=walk(x.box).find(el=>el.children?.some(child=>child.textContent==='Select all shown suggestions')).children[0];selectAll.checked=true;selectAll.listeners.change();assert.ok(proposals.every(row=>row.children[0].checked));
 await find(x.box,'Use selected suggestions').listeners.click();assert.equal(x.inputs[0].value,'Fictional Current');assert.equal(x.inputs[2].value,'Lead');assert.equal(x.inputs[1].value,'Keep manual answer');assert.equal(x.inputs[0]._historySource.entry_id,'a'.repeat(32));assert.equal(x.inputs[2]._historySource.entry_id,'b'.repeat(32));
 assert.ok(x.calls.every(call=>!call.path.includes('/fill')));
 const z=fixture({empty:true}),stopped=find(z.box,'Suggest history answers for blank fields').listeners.click();
 for(let i=0;i<20&&z.calls.length<3;i++)await Promise.resolve();await find(z.box,'Stop preparing answers').listeners.click();await stopped;
 assert.equal(find(z.box,'Why some answers need review').open,undefined);const diagnostic=walk(z.box).find(el=>el.children?.some(child=>child.textContent==='Why some answers need review'));assert.equal(diagnostic.open,true);
 console.log('Whole-form history blank-field filtering, progress, cancellation, source review and explicit multi-selection passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
