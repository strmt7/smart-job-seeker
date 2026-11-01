const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const line = source.split('\n').find(item => item.startsWith('function showTemplateVision('));
function node(tag, text = '') { return {tag, text, children:[], listeners:{}, append(...items){this.children.push(...items)}, setAttribute(){}, addEventListener(event, fn){this.listeners[event]=fn}}; }
const calls = [], saved = new Map(); let followed;
const context = {TextEncoder, element:node, document:{createElement:node}, window:{confirm:()=>true}, notify:message=>{throw Error(message)}, draftPriorities:new Map(), api:async(path,body)=>{calls.push({path,body});return {task_id:'new-task'}}, sessionStorage:{setItem:(key,value)=>saved.set(key,value)}, followDraft:async(task,job)=>{followed={task,job}}};
const render = vm.runInNewContext(`(${line})`,context);
(async()=>{
 const panel=node('div'),draft={kind:'cv',template_pdf_coverage:{pages_without_selectable_text:[1,2,3],structurally_blank_pages:[2]}};
 render(panel,{page:1,pages:3,text:'Old',image:'abc',other_pages_not_transcribed:2},{id:'job'},draft,'old-task');
 const details=panel.children[0],label=details.children.find(item=>item.tag==='label'),select=label.children[0];
 assert.equal(select.children.length,1);assert.equal(select.children[0].value,'3');select.value='3';
 const button=details.children.find(item=>item.text==='Redraft using selected page');
 await button.listeners.click();assert.equal(calls[0].body.template_scan_page,3);assert.equal(calls[0].body.template_draft_id,'old-task');assert.deepEqual(followed,{task:'new-task',job:'job'});assert.ok(saved.has('waypoint-draft'));
 const keep=details.children.find(item=>item.text==='Keep reviewed page and add selected page');
 await keep.listeners.click();assert.equal(calls[1].body.template_scan_page,3);assert.equal(calls[1].body.reviewed_template_text,'Old');assert.equal(calls[1].body.template_reviewed,true);assert.equal(calls[1].body.retain_reviewed_pages,true);
 const full=node('div');render(full,{page:3,pages:4,text:'Current',image:'abc',other_pages_not_transcribed:1,reviewed_pages:[{page:1,text:'First reviewed'},{page:2,text:'Second reviewed'}]},{id:'job'},{kind:'cv',template_pdf_coverage:{pages_without_selectable_text:[1,2,3,4],structurally_blank_pages:[]}},'new-task');
 const fullDetails=full.children[0];assert.ok(fullDetails.children.some(item=>item.text==='First reviewed'));assert.ok(!fullDetails.children.some(item=>item.text==='Keep reviewed page and add selected page'));
 console.log('Scanned template page selection UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
