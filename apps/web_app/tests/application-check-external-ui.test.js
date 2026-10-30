const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const src=fs.readFileSync('apps/web_app/web/app.js','utf8');
const begin=src.indexOf("if(job.canonical_url && job.source!=='jobicy'){");
const end=src.indexOf('side.append(element(',begin);
assert.ok(begin>=0&&end>begin);
function node(tag,text=''){return {tag,textContent:text,children:[],listeners:{},checked:false,append(...x){this.children.push(...x);},addEventListener(event,fn){this.listeners[event]=fn;}};}
async function test(enabled){const details=node('div'),calls=[];const context={job:{id:'fixture',canonical_url:'https://example.test/job',source:'user_supplied_url'},details,pendingApplicationCheck:()=>null,element:node,document:{createElement:tag=>node(tag),createTextNode:text=>node('text',text)},api:async(path,payload)=>{calls.push({path,payload});return {task_id:'task'};},sessionStorage:{setItem(){},removeItem(){}},followApplicationCheck:async()=>{},notify(){},renderJobs(){},recoverPendingApplicationCheck:async()=>{}};
 vm.runInNewContext(src.slice(begin,end),context);
 const label=details.children[0],box=label.children[0],button=details.children[2];assert.equal(box.checked,false);box.checked=enabled;await button.listeners.click();assert.equal(calls.length,1);assert.equal(calls[0].payload.allow_external,enabled);}
(async()=>{await test(false);await test(true);console.log('External application check UI checks passed.');})().catch(e=>{console.error(e);process.exitCode=1;});
