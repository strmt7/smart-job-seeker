const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
function element(tag,text=''){return {tagName:tag.toUpperCase(),textContent:text,value:'',children:[],listeners:{},isConnected:true,append(...items){this.children.push(...items);},replaceChildren(...items){this.children=items;},setAttribute(key,value){(this.attributes??={})[key]=value;},addEventListener(key,handler){this.listeners[key]=handler;},dispatchEvent(event){this.listeners[event.type]?.(event);},get options(){return this.children;}};}
const walk=root=>[root,...root.children.flatMap(item=>item?.children?walk(item):[])];
function fixture(){
 const calls=[],notices=[],input=element('input');input._draftId='old';
 const context={document:{createElement:element,createTextNode:text=>({textContent:text})},Option:function(text,value){return {text,value};},Event:class{constructor(type){this.type=type;}},TextEncoder,CandidateHistory:{parse:()=>({employment:[{id:'a'.repeat(32),employer:'Fictional Co',title:'Engineer',reviewed:true}],education:[{id:'b'.repeat(32),institution:'Unchecked',reviewed:false}]})},notify:message=>notices.push(message),api:async(path,body)=>{calls.push({path,body});if(path.endsWith('-start'))return {task_id:'task'};if(path.endsWith('-pending'))return {active:false};return {status:'complete',advice:{entry_label:'Engineer at Fictional Co',proposals:[{selector:'#company',label:'Employer',value:{text:'Fictional Co'},entry_id:'a'.repeat(32),component:'employer',source_sha256:'c'.repeat(64)}],manual_review:[]}};}};
 vm.createContext(context);vm.runInContext(fs.readFileSync('apps/web_app/web/app.js','utf8').split('\n').filter(line=>line.startsWith('function clearFormHistorySource(')||line.startsWith('function linkFormHistorySource(')).join('\n'),context);vm.runInContext(fs.readFileSync('apps/web_app/web/history-mapping.js','utf8'),context);
 const box=context.HistoryMapping.control({session_id:'session',fields:[{selector:'#company',label:'Employer',field_type:'text',sensitive:false},{selector:'#private',label:'Sensitive',field_type:'text',sensitive:true}]},[{selector:'#company',input,kind:'text'}],{});
 const entry=walk(box).find(el=>el.tagName==='SELECT'),scope=walk(box).find(el=>el.tagName==='FIELDSET'),generate=walk(box).find(el=>el.textContent==='Suggest answers');
 return {box,entry,scope,generate,input,calls,notices};
}
(async()=>{
 const x=fixture();await x.generate.listeners.click();assert.equal(x.calls.length,0);
 assert.equal(x.entry.options.length,2);assert.equal(walk(x.scope).filter(el=>el.type==='checkbox').length,1);
 x.entry.value='a'.repeat(32);walk(x.scope).find(el=>el.type==='checkbox').checked=true;
 await x.generate.listeners.click();assert.equal(x.input.value,'');assert.equal(x.input._draftId,'old');assert.equal(x.calls[0].body.selectors[0],'#company');
 let proposal=walk(x.box).find(el=>el.className==='history-proposal');proposal.children[0].checked=true;
 let apply=walk(x.box).find(el=>el.textContent==='Use selected suggestions');
 x.input.value='New manual edit';await apply.listeners.click();assert.equal(x.input.value,'New manual edit');assert.match(x.notices.at(-1),/edits were kept/);
 const y=fixture();y.entry.value='a'.repeat(32);walk(y.scope).find(el=>el.type==='checkbox').checked=true;await y.generate.listeners.click();
 proposal=walk(y.box).find(el=>el.className==='history-proposal');proposal.children[0].checked=true;apply=walk(y.box).find(el=>el.textContent==='Use selected suggestions');await apply.listeners.click();
 assert.equal(y.input.value,'Fictional Co');assert.equal(y.input._draftId,undefined);assert.equal(y.input._historySource.entry_id,'a'.repeat(32));assert.equal(y.input._historySourceValue,'Fictional Co');y.input.value='Edited';y.input.dispatchEvent({type:'input'});assert.equal(y.input._historySource,undefined);y.input.value='Fictional Co';assert.equal(y.input._historySource,undefined);assert.ok(y.calls.every(call=>!call.path.includes('/fill')));
 const z=fixture();z.input.value='Existing answer';z.entry.value='a'.repeat(32);walk(z.scope).find(el=>el.type==='checkbox').checked=true;await z.generate.listeners.click();
 proposal=walk(z.box).find(el=>el.className==='history-proposal');assert.equal(proposal.children[0].checked,false);z.entry.listeners.change();assert.ok(!walk(z.box).some(el=>el.textContent==='Use selected suggestions'));
 console.log('History advice review, explicit selection and edit-preservation checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
