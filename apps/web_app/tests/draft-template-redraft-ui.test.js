const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const line=source.split('\n').find(line=>line.startsWith('function showTemplateVision('));
function node(tag,text='',className=''){return {tag,text,className,children:[],listeners:{},append(...items){this.children.push(...items)},setAttribute(){},addEventListener(event,fn){this.listeners[event]=fn}}}
const calls=[],saved=new Map();let confirmed=false,followed=null;
const context={TextEncoder,element:node,document:{createElement:node},window:{confirm:()=>confirmed},notify:()=>{},draftPriorities:new Map([['job',new Set(['fact'])]]),api:async(path,body)=>{calls.push({path,body});return {task_id:'new-task'}},sessionStorage:{setItem:(key,value)=>saved.set(key,value)},followDraft:async(task,job)=>{followed={task,job}}};
const render=vm.runInNewContext('('+line+')',context);
(async()=>{const panel=node('div');render(panel,{page:1,pages:1,text:'Old',image:'abc',other_pages_not_transcribed:0},{id:'job'},{kind:'cv'},'old-task');const details=panel.children[0],text=details.children.find(item=>item.tag==='textarea'),button=details.children.find(item=>item.tag==='button');text.value='Corrected';await button.listeners.click();assert.equal(calls.length,0);confirmed=true;await button.listeners.click();assert.equal(calls.length,1);assert.equal(calls[0].body.reviewed_template_text,'Corrected');assert.equal(calls[0].body.template_draft_id,'old-task');assert.equal(calls[0].body.template_reviewed,true);assert.equal(calls[0].body.priority_ids[0],'fact');assert.equal(followed.task,'new-task');assert.equal(followed.job,'job');assert.ok(saved.has('waypoint-draft'));console.log('Reviewed transcription redraft UI checks passed.');})().catch(error=>{console.error(error);process.exitCode=1});
