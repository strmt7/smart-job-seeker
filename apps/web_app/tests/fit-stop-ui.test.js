const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),test=require('node:test');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf(" assess.type='button';"),end=source.indexOf('\n box.append(assess,stop,output);return box;',start);
assert.ok(start>=0 && end>start);
const flush=()=>new Promise(setImmediate);
function fixture(cancel){
 const timers=[],calls=[],messages=[],storage=new Map(),buttons=[];
 const element=(tag,text,cls)=>{const node={tag,textContent:text,className:cls,listeners:{},addEventListener(e,fn){this.listeners[e]=fn;}};if(tag==='button')buttons.push(node);return node;};
 const assess=element('button','Assess'),output={children:[],replaceChildren(...children){this.children=children;},append(child){this.children.push(child);}};
 let statuses=[],saved=0,rescans=0,starts=0;
 const context={assess,output,element,activeTask:null,autoFitRunning:false,attentionSnapshot:false,row:{job:{id:'fictional-job'}},state:{jobs:[]},box:{append(){}},sessionStorage:{getItem:k=>storage.get(k)??null,setItem:(k,v)=>storage.set(k,v),removeItem:k=>storage.delete(k)},setTimeout:fn=>timers.push(fn),notify:m=>messages.push(m),scheduleAutomaticFit:()=>rescans++,showAssessment:()=>{saved++;output.replaceChildren(element('p','Saved result'));},api:async(path,body)=>{calls.push([path,body]);if(path==='/api/fit/pending')return {active:false};if(path==='/api/fit/start')return {task_id:`owned-task-${++starts}`};if(path==='/api/fit/cancel')return cancel();if(path==='/api/fit/status'){assert.ok(statuses.length,'Unexpected poll');return statuses.shift();}throw Error('Unexpected API '+path);}};
 vm.runInNewContext(source.slice(start,end),context);
 return {assess,output,calls,messages,storage,timers,stop:buttons[1],setStatuses:s=>statuses=s,get saved(){return saved;},get rescans(){return rescans;}};
}
test('manual Stop uses task identity, shows stopping, clears resume state and never displays a stopped score',async()=>{
 const f=fixture(async()=>({status:'stopping'}));
 const operation=f.assess.listeners.click();await flush();
 assert.equal(f.stop.hidden,false);assert.equal(f.assess.disabled,true);assert.match(f.output.children[0].textContent,/Checking/);
 await f.stop.listeners.click();assert.equal(f.stop.disabled,true);assert.match(f.output.children[0].textContent,/Stopping/);
 assert.equal(f.calls.find(([p])=>p==='/api/fit/cancel')[1].task_id,'owned-task-1');
 f.setStatuses([{status:'running',phase:'stopping'},{status:'stopped'}]);
 f.timers.shift()();await flush();assert.match(f.output.children[0].textContent,/Stopping/);
 f.timers.shift()();await operation;
 assert.match(f.output.children[0].textContent,/Assessment stopped/);assert.equal(f.saved,0);assert.equal(f.storage.size,0);assert.equal(f.stop.hidden,true);assert.equal(f.assess.disabled,false);assert.equal(f.rescans,1);
});
test('late cancel reply cannot overwrite a collected assessment',async()=>{
 let reply;const f=fixture(()=>new Promise(resolve=>reply=resolve));
 const operation=f.assess.listeners.click();await flush();
 const stopping=f.stop.listeners.click();f.setStatuses([{status:'complete',assessment:{score:75}}]);
 f.timers.shift()();await operation;assert.equal(f.saved,1);
 reply({status:'finished'});await stopping;
 assert.equal(f.output.children[0].textContent,'Saved result');assert.equal(f.stop.hidden,true);
});
test('failed Stop can be retried while the active assessment remains collectable',async()=>{
 const f=fixture(async()=>{throw Error('Temporary stop failure');});
 const operation=f.assess.listeners.click();await flush();await f.stop.listeners.click();
 assert.equal(f.stop.disabled,false);assert.deepEqual(f.messages,['Temporary stop failure']);
 f.setStatuses([{status:'complete',assessment:{score:75}}]);f.timers.shift()();await operation;assert.equal(f.saved,1);
});

test('reply for an earlier Stop cannot change a newly started assessment',async()=>{
 let reply;const f=fixture(()=>new Promise(resolve=>reply=resolve));
 const first=f.assess.listeners.click();await flush();const oldStop=f.stop.listeners.click();
 f.setStatuses([{status:'complete',assessment:{score:75}}]);f.timers.shift()();await first;
 const second=f.assess.listeners.click();await flush();assert.match(f.output.children[0].textContent,/Checking/);
 reply({status:'finished'});await oldStop;
 assert.match(f.output.children[0].textContent,/Checking/);assert.equal(f.stop.disabled,false);assert.equal(f.stop.hidden,false);
 f.setStatuses([{status:'complete',assessment:{score:80}}]);f.timers.shift()();await second;assert.equal(f.saved,2);
});
