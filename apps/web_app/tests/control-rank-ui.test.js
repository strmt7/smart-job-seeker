const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf(" const controlsBox=element('details'"),end=source.indexOf(" const visualPanel=element('div'",start);
assert.ok(start>=0 && end>start);
async function run(failure=false){
 const created=[],requests=[],storage=new Map();
 function element(tag,text=''){const node={tag,text,children:[],isConnected:true,disabled:false,append(...items){this.children.push(...items)},replaceChildren(...items){this.children=items},addEventListener(_type,fn){this.handler=fn}};created.push(node);return node;}
 const context={element,panel:{append(){}},result:{session_id:'fixture'},setTimeout:fn=>fn(),sessionStorage:{getItem:k=>storage.get(k)||null,setItem:(k,v)=>storage.set(k,v),removeItem:k=>storage.delete(k)},api:async(path,body)=>{requests.push([path,body]);if(path.endsWith('pending'))return {active:false};if(path.endsWith('start'))return {task_id:'task'};return failure?{status:'failed',error:'The page changed'}:{status:'complete',controls:{incomplete:true,controls:[{label:'Continue',disabled:false},{label:'Submit application',disabled:false}]},indices:[0]};}};
 vm.runInNewContext(source.slice(start,end),context);
 const button=created.find(item=>item.text==='Suggest section controls with local AI');await button.handler();
 const status=created.find(item=>item.tag==='div');const text=status.children.map(item=>item.text).join('\n');
 assert.equal(button.disabled,false);assert.ok(requests.every(([path])=>!path.includes('click')&&!path.includes('submit')));
 return text;
}
(async()=>{const text=await run();assert.match(text,/Suggested · Continue/);assert.match(text,/Submit application/);assert.match(text,/inventory was incomplete/);assert.match(await run(true),/page changed.*Inspect the page manually/);console.log('Control advice UI checks passed.');})().catch(error=>{console.error(error);process.exitCode=1;});
