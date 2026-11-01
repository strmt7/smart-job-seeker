const assert = require('node:assert/strict'), fs = require('node:fs'), vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const line = prefix => { const found=source.split('\n').find(s=>s.startsWith(prefix)); assert.ok(found); return found; };
let current=true, notice='';
const field={value:'',dataset:{},closest:()=>({open:false}),focus(){}};
const box={replaceChildren(...children){this.children=children}};
const context={document:{querySelector:()=>field}, sessionStorage:{removeItem(){}},
 element:(_tag,text)=>({text}), notify:text=>{notice=text}, api:async()=>({current})};
vm.runInNewContext(`${line('function searchPlanBinding(')}\n${line('async function appendSearchPlanToWebBatch(')}`,context);
(async()=>{
 const button={disabled:false};
 await context.appendSearchPlanToWebBatch('Fictional engineer','digest',box,button);
 await context.appendSearchPlanToWebBatch('Fictional developer','digest',box,button);
 assert.equal(field.value,'Fictional engineer\nFictional developer');
 assert.equal(context.searchPlanBinding(field),'digest');
 await context.appendSearchPlanToWebBatch('fictional engineer','digest',box,button);
 assert.match(notice,/already/);
 field.value+='\nmanual revision';assert.equal(context.searchPlanBinding(field),null);
 field.value=Array.from({length:10},(_,i)=>`role ${i}`).join('\n');
 await context.appendSearchPlanToWebBatch('another role','digest',box,button);assert.match(notice,/at most 10/);
 current=false;await context.appendSearchPlanToWebBatch('role','digest',box,button);assert.match(box.children[0].text,/changed/);
 console.log('Search-plan web batch checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1});
