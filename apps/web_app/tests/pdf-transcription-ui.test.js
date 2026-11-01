const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf('async function transcribePdfPage(');
const end=source.indexOf('\nfunction pendingDocumentVisual(',start);
assert.ok(start>=0 && end>start);
function node(tag,text=''){return {tag,text,children:[],listeners:{},append(...items){this.children.push(...items)},addEventListener(event,fn){this.listeners[event]=fn},setAttribute(){},focus(){}}}
const saved=new Map(),calls=[],panel=node('div'),button=node('button'),input=node('input');let selectedRegion='top';
const context={TextEncoder,documentReviewGeneration:4,element:node,notify(){},setWorkspaceView(){},document:{createElement:node,querySelector:()=>input},sessionStorage:{getItem:key=>saved.get(key),setItem:(key,value)=>saved.set(key,value),removeItem:key=>saved.delete(key)},setTimeout:fn=>fn(),api:async(path,body)=>{calls.push({path,body});if(path.endsWith('pending'))return {active:false};if(path.endsWith('start')){selectedRegion=body.region;return {task_id:'task'}};return {status:'complete',result:{page:1,pages:2,region:selectedRegion,text:'Fictional qualification',uncertain:true,image:'png'}}}};
const run=vm.runInNewContext(source.slice(start,end)+'\ntranscribePdfPage',context);
(async()=>{await run({id:'doc'},1,panel,4,button,'top');assert.equal(calls.length,3);assert.equal(calls[1].body.region,'top');assert.ok(calls.every(call=>!call.path.includes('/facts')));const section=panel.children[0];assert.match(section.children[0].text,/top half/);assert.match(section.children[1].text,/unverified model output.*uncertainty/i);const textarea=section.children.find(item=>item.tag==='textarea');textarea.selectionStart=0;textarea.selectionEnd=textarea.value.length;section.children.find(item=>item.tag==='button').listeners.click();assert.equal(input.value,'Fictional qualification');assert.ok(calls.every(call=>!call.path.includes('/facts')));assert.equal(saved.size,0);await run({id:'doc'},1,panel,4,button,'right');assert.equal(calls[4].body.region,'right');assert.match(panel.children[1].children[0].text,/right half/);assert.match(panel.children[1].children.find(item=>item.tag==='img').alt,/right half/);console.log('PDF transcription UI checks passed.');})().catch(error=>{console.error(error);process.exitCode=1});
