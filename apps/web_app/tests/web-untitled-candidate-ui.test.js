const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const candidate=source.split('\n').find(line=>line.startsWith('function webCandidate('));
assert.ok(candidate);
function node(tag,text=''){return {tag,textContent:text,children:[],listeners:{},append(...items){this.children.push(...items)},addEventListener(event,fn){this.listeners[event]=fn},setAttribute(){}};}
const form={elements:Object.fromEntries(['url','title','employer','location','requisition_id','description'].map(name=>[name,{value:'',focus(){}}])),closest:()=>({open:false}),scrollIntoView(){}};
const context={selectedCandidates:new Map(),element:node,notify(){},updateCandidateSelection(){},document:{querySelector:()=>form,createElement:node,createTextNode:text=>node('text',text)}};
const render=vm.runInNewContext(candidate+'\nwebCandidate',context);
const card=render({url:'https://example.org/role',title:'Untitled result from example.org',title_missing:true,snippet:''});
card.children.find(item=>item.textContent==='Enter details manually').listeners.click();
assert.equal(form.elements.url.value,'https://example.org/role');
assert.equal(form.elements.title.value,'');
console.log('Untitled candidate UI checks passed.');
