const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf('let discoveryFollowing=false;'),end=source.indexOf("document.querySelector('#discovery').addEventListener",start);
assert.ok(start>=0 && end>start);
const nodes=new Map(),storage=new Map(),messages=[];
function node(id){if(!nodes.has(id))nodes.set(id,{textContent:'',hidden:false,children:[],replaceChildren(...items){this.children=items;},append(item){this.children.push(item);}});return nodes.get(id);}
const history=[{source:'greenhouse',board:'fixture',summary:'1 saved'},{source:'lever',board:'missing',summary:'1 degraded'}];let polls=0,pendingActive=true;
const context={document:{querySelector:node},sessionStorage:{getItem:k=>storage.get(k)||null,setItem:(k,v)=>storage.set(k,v),removeItem:k=>storage.delete(k)},element:(_tag,text)=>({text}),notify:msg=>messages.push(msg),refresh:async()=>{},setTimeout:fn=>fn(),api:async(path)=>{
 if(path.endsWith('/start'))throw Error('Lost start response');
 if(path.endsWith('/pending'))return pendingActive?{active:true,task_id:'fixture'}:{active:false,last:{task_id:'fixture'}};
 if(path.endsWith('/status')){polls++;if(polls===1)throw Error('Lost status response');return polls===2?{status:'running',processed:1,total:2,history:history.slice(0,1)}:{status:'complete',processed:2,total:2,cancelled:false,history};}
 throw Error('Unexpected request');
}};
(async()=>{vm.runInNewContext(source.slice(start,end),context);await vm.runInNewContext('startDiscovery',context)([]);assert.equal(storage.get('waypoint-discovery'),'fixture');assert.equal(node('#discovery-resume').hidden,false);await vm.runInNewContext('recoverDiscovery',context)();assert.equal(polls,3);assert.deepEqual(node('#discovery-results').children.map(x=>x.text),['greenhouse:fixture — 1 saved','lever:missing — 1 degraded']);assert.equal(node('#discovery-progress').textContent,'Finished 2 of 2 boards.');assert.equal(storage.has('waypoint-discovery'),false);assert.ok(messages.includes('Lost start response'));assert.ok(messages.includes('Lost status response'));pendingActive=false;storage.clear();await vm.runInNewContext('recoverDiscovery',context)();assert.equal(polls,4);assert.equal(node('#discovery-progress').textContent,'Finished 2 of 2 boards.');await vm.runInNewContext('recoverDiscovery',context)();assert.equal(polls,4);console.log('Board discovery recovery UI checks passed.');})().catch(error=>{console.error(error);process.exitCode=1;});
