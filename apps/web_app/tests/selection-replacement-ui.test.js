const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf(' const reviewableSelects=');
const end=source.indexOf('\n for(const reason of result.manual_review)',start);
assert.ok(start>=0 && end>start);
function node(tag,text=''){return {tag,textContent:text,children:[],value:'',checked:false,disabled:false,append(...items){this.children.push(...items);},addEventListener(name,fn){this[name]=fn;},setAttribute(){}};}
(async()=>{
 let pending;const panel=node('panel'), approvals=new Map(), desired=new Map([['#choice','Beta']]);
 const ctx={result:{session_id:'session',fields:[{selector:'#choice',label:'Work arrangement',sensitive:false,field_type:{select:{options:['Alpha','Beta']}}}]},panel,replacementApprovals:approvals,desiredSelection:s=>desired.get(s)||'',document:{createElement:node,createTextNode:text=>node('text',text)},element:node,Option:function(text,value){this.textContent=text;this.value=value;},notify(){},api:()=>new Promise(resolve=>{pending=resolve;})};
 vm.runInNewContext(source.slice(start,end),ctx);
 const details=panel.children[0],pick=details.children.find(x=>x.tag==='select'),inspect=details.children.find(x=>x.textContent==='Inspect current selection'),message=details.children.find(x=>x.textContent.startsWith('Choose a dropdown')),ack=details.children.find(x=>x.tag==='label').children[0],status=details.children.find(x=>x.textContent==='No replacements approved.');
 pick.value='#choice';const request=inspect.click();pending({selection:{label:'Alpha',value:'a'},digest:'a'.repeat(64)});await request;
 assert.match(message.textContent,/Work arrangement: currently selected Alpha/);assert.equal(ack.disabled,false);
 ack.checked=true;ack.change();assert.equal(approvals.get('#choice').expected,'Beta');assert.match(status.textContent,/Work arrangement/);
 desired.set('#choice','Alpha');const next=inspect.click();pick.value='';pick.change();pending({selection:{label:'Alpha',value:'a'},digest:'b'.repeat(64)});await next;
 assert.equal(ack.disabled,true);assert.match(message.textContent,/changed; inspect again/);
 console.log('Selection replacement review UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
