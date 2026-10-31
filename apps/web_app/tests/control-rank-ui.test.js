const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf(" const controlsBox=element('details'"),end=source.indexOf(" const visualPanel=element('div'",start);
assert.ok(start>=0 && end>start);
async function run(failure=false){
 const created=[],requests=[],storage=new Map();let refreshed=0;
 function element(tag,text=''){const node={tag,text,children:[],isConnected:true,disabled:false,append(...items){this.children.push(...items)},replaceChildren(...items){this.children=items},addEventListener(_type,fn){this.handler=fn}};created.push(node);return node;}
 const context={element,document:{createElement:tag=>element(tag)},panel:{append(){}},visualPanel:{replaceChildren(){}},visualButton:{disabled:false},fillButton:null,fillInProgress:false,fillCompleted:false,result:{session_id:'fixture'},setTimeout:fn=>fn(),refresh:async()=>{refreshed++},sessionStorage:{getItem:k=>storage.get(k)||null,setItem:(k,v)=>storage.set(k,v),removeItem:k=>storage.delete(k)},api:async(path,body)=>{requests.push([path,body]);if(path.endsWith('pending'))return {active:false};if(path.endsWith('start'))return {task_id:'task'};if(path.endsWith('control-vision-status'))return {status:'complete',comparison:'not_observed',buttons:['Next'],screenshot:'fixture',capture_saved:false,result_saved:false};if(path.endsWith('control-focus'))return {screenshot:'fixture',record_saved:true,page_unchanged:true,verified_count:0,unverified_count:0,failed_fields:['First name'],failed_attachments:0,unverified_attachments:0};return failure?{status:'failed',error:'The page changed'}:{status:'complete',controls:{incomplete:true,controls:[{label:'Continue',disabled:false},{label:'Submit application',disabled:false}]},indices:[0]};}};
 vm.runInNewContext(source.slice(start,end),context);
 const button=created.find(item=>item.text==='Suggest section controls with local AI');await button.handler();
 const status=created.find(item=>item.tag==='div');const text=status.children.flatMap(item=>[item.text,...item.children.map(child=>child.text)]).join('\n');
 if(!failure){const locate=created.find(item=>item.text==='Locate on page');await locate.handler();assert.equal(requests.at(-1)[0],'/api/form/control-focus');assert.equal(requests.at(-1)[1].index,0);assert.equal(refreshed,1);assert.ok(created.some(item=>item.text?.includes('1 need review')));const inspect=created.find(item=>item.text==='Inspect control visually');await inspect.handler();assert.equal(requests.at(-1)[0],'/api/form/control-vision-status');assert.ok(created.some(item=>item.text?.includes('did not transcribe')));assert.ok(created.some(item=>item.text?.includes('not saved')));assert.equal(refreshed,2);}
 assert.equal(button.disabled,false);assert.ok(requests.every(([path])=>!path.includes('click')&&!path.includes('submit')));
 if(failure)assert.equal(refreshed,1);
 return text;
}
(async()=>{const text=await run();assert.match(text,/Suggested · Continue/);assert.match(text,/Submit application/);assert.match(text,/inventory was incomplete/);assert.match(await run(true),/page changed.*Inspect the page manually/);console.log('Control advice UI checks passed.');})().catch(error=>{console.error(error);process.exitCode=1;});
