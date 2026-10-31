const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf(' const reviewableFields=');
const end=source.indexOf('\n for(const reason of result.manual_review)',start);
assert.ok(start>=0 && end>start);
function node(tag,text=''){return {tag,textContent:text,style:{},children:[],value:'',checked:false,disabled:false,append(...items){this.children.push(...items);},addEventListener(name,fn){this[name]=fn;},setAttribute(){}};}
(async()=>{
 let pending;const panel=node('panel'), approvals=new Map(), desired=new Map([['#choice','Beta'],['#text','New answer'],['#multi','["Beta"]'],['#check','false']]);
 const ctx={result:{session_id:'session',fields:[{selector:'#choice',label:'Work arrangement',sensitive:false,field_type:{select:{options:['Alpha','Beta']}}},{selector:'#text',label:'Notes',sensitive:false,field_type:'text_area'},{selector:'#multi',label:'Skills',sensitive:false,field_type:{multi_select:{options:['Alpha','Beta']}}},{selector:'#check',label:'Travel',sensitive:false,field_type:'checkbox'}]},panel,replacementApprovals:approvals,desiredSelection:s=>desired.get(s)||'',document:{createElement:node,createTextNode:text=>node('text',text)},element:node,Option:function(text,value){this.textContent=text;this.value=value;},notify(){},api:()=>new Promise(resolve=>{pending=resolve;})};
 vm.runInNewContext(source.slice(start,end),ctx);
 const details=panel.children[0],pick=details.children.find(x=>x.tag==='select'),inspect=details.children.find(x=>x.textContent==='Inspect current answer'),message=details.children.find(x=>x.textContent.startsWith('Choose a field')),ack=details.children.find(x=>x.tag==='label').children[0],status=details.children.find(x=>x.textContent==='No replacements approved.');
 pick.value='#choice';const request=inspect.click();pending({selection:{label:'Alpha',value:'a'},digest:'a'.repeat(64)});await request;
 assert.match(message.textContent,/Work arrangement: review/);assert.match(details.children.find(x=>x.tag==='pre').textContent,/Current value:\nAlpha/);assert.equal(ack.disabled,false);
 ack.checked=true;ack.change();assert.equal(approvals.get('#choice').expected,'Beta');assert.match(status.textContent,/Work arrangement/);
 desired.set('#choice','Alpha');const next=inspect.click();pick.value='';pick.change();pending({selection:{label:'Alpha',value:'a'},digest:'b'.repeat(64)});await next;
 assert.equal(ack.disabled,true);assert.match(message.textContent,/changed; inspect again/);assert.equal(approvals.has('#choice'),false);
 pick.value='#text';const textRequest=inspect.click();pending({text:{value:'Old answer\nSecond line'},digest:'c'.repeat(64)});await textRequest;assert.match(details.children.find(x=>x.tag==='pre').textContent,/Old answer\nSecond line/);ack.checked=true;ack.change();assert.equal(approvals.get('#text').expected,'New answer');
 pick.value='#multi';const multiRequest=inspect.click();pending({choice:{kind:'multi',checked:null,selections:[{label:'Alpha',value:'a'}]},digest:'d'.repeat(64)});await multiRequest;assert.match(details.children.find(x=>x.tag==='pre').textContent,/Alpha.*\n\nReviewed replacement:\nBeta/);ack.checked=true;ack.change();assert.equal(approvals.get('#multi').expected,'["Beta"]');
 pick.value='#check';const checkRequest=inspect.click();pending({choice:{kind:'checkbox',checked:true,selections:[]},digest:'e'.repeat(64)});await checkRequest;assert.match(details.children.find(x=>x.tag==='pre').textContent,/Checked\n\nReviewed replacement:\nUnchecked/);ack.checked=true;ack.change();assert.equal(approvals.get('#check').expected,'false');
 console.log('Selection replacement review UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
