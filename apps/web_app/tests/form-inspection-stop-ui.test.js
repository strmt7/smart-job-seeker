const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),test=require('node:test');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8').replace(/\r\n/g,'\n'),start=source.indexOf('async function followFormInspection('),end=source.indexOf('\n}\n',start)+2;
assert.ok(start>=0&&end>start);
const flush=()=>new Promise(setImmediate);
function fixture(cancel){
 const timers=[],calls=[],messages=[],storage=new Map(),buttons=[];
 const element=(tag,text)=>{const node={textContent:text,listeners:{},addEventListener(event,fn){this.listeners[event]=fn;}};if(tag==='button')buttons.push(node);return node;};
 const panel={children:[],replaceChildren(...children){this.children=children;}};
 let statuses=[],previews=0;
 const context={formInspectionFollowing:false,document:{querySelector:()=>panel},element,sessionStorage:{getItem:k=>storage.get(k)??null,removeItem:k=>storage.delete(k)},reviewQueue:[],saveReviewQueue(){},state:{jobs:[{job:{id:'job'}}]},notify:m=>messages.push(m),refresh:async()=>{},showFormPreview(){previews++;},setTimeout:fn=>timers.push(fn),api:async(path,body)=>{calls.push([path,body]);if(path==='/api/form/inspect-cancel')return cancel();if(path==='/api/form/inspect-status'){assert.ok(statuses.length);return statuses.shift();}throw Error(path);}};
 vm.runInNewContext(source.slice(start,end),context);
 return {context,panel,buttons,calls,messages,storage,timers,setStatuses:s=>statuses=s,get previews(){return previews;},follow:(id='task')=>context.followFormInspection(id,'job')};
}
test('inspection Stop targets its task, preserves unknown progress and never opens a cancelled preview',async()=>{
 const f=fixture(async()=>({status:'stopping'}));f.storage.set('waypoint-form-inspection',JSON.stringify({task:'task',job:'job'}));
 const operation=f.follow();operation.catch(()=>{});await flush();
 assert.match(f.panel.children[0].textContent,/Checking/);const stop=f.buttons[0];await stop.listeners.click();
 assert.equal(f.calls.find(([p])=>p==='/api/form/inspect-cancel')[1].task_id,'task');assert.equal(stop.disabled,true);
 f.setStatuses([{status:'running',phase:'stopping'},{status:'failed',error:'Form inspection was stopped'}]);
 f.timers.shift()();await flush();assert.match(f.panel.children[0].textContent,/Stopping/);
 f.timers.shift()();await assert.rejects(operation,/was stopped/);
 assert.equal(f.context.formInspectionFollowing,false);assert.equal(f.previews,0);assert.equal(f.panel.children.length,0);assert.equal(f.storage.size,0);
});
test('a late Stop reply cannot replace another inspection progress',async()=>{
 let reply;const f=fixture(()=>new Promise(resolve=>reply=resolve));
 const first=f.follow();await flush();const oldStop=f.buttons[0].listeners.click();
 f.setStatuses([{status:'complete'}]);f.timers.shift()();await first;
 const second=f.follow('new-task');await flush();
 reply({status:'finished'});await oldStop;assert.match(f.panel.children[0].textContent,/Checking/);
 f.setStatuses([{status:'running',phase:'opening'},{status:'complete'}]);f.timers.shift()();await flush();assert.match(f.panel.children[0].textContent,/Opening/);
 f.timers.shift()();await second;assert.equal(f.previews,2);
});
test('failed inspection Stop remains retryable',async()=>{
 const f=fixture(async()=>{throw Error('Temporary control error');});const work=f.follow();await flush();
 const stop=f.buttons[0];await stop.listeners.click();assert.equal(stop.disabled,false);assert.deepEqual(f.messages,['Temporary control error']);
 f.setStatuses([{status:'complete'}]);f.timers.shift()();await work;assert.equal(f.previews,1);
});
