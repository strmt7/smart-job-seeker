const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8'),helpers=source.slice(source.indexOf('function visualComparisonStatus('),source.indexOf('function showAutomaticFit(')),focus=source.slice(source.indexOf('async function focusFieldVision('),source.indexOf('\n fieldVisionButton.addEventListener',source.indexOf('async function focusFieldVision('))),visual=source.slice(source.indexOf(' async function inspectVisual('),source.indexOf('\n async function reviewAfterFillFailure(',source.indexOf(' async function inspectVisual(')));
function element(tag,text=''){return {tag,text,children:[],listeners:{},isConnected:true,setAttribute(name,value){(this.attributes??={})[name]=value;},append(...items){this.children.push(...items)},replaceChildren(...items){this.children=[...items]},addEventListener(name,fn){this.listeners[name]=fn}}}
const walk=root=>[root,...root.children.flatMap(child=>walk(child))];
async function run(options={}){
 const {mismatches=3,coverage=true,retained=true,focusSaved=true,statusSaved=true,fresh='same',focusFresh='same',badFocused=false,cancel=false,detach=false,alreadySaved=false,unobserved=false,duplicate=false,domUnconfirmed=false,expire=false,running=false}=options;
 const calls=[],root=element('div'),storage=new Map(),expected=Array.from({length:mismatches+1},(_,i)=>({label:`Employment ${i} — Company`,expected:`Fictional ${i}`,dom_status:domUnconfirmed&&i===0?'unconfirmed':'matches'})),fields=expected.map((item,i)=>({label:item.label,selector:`#f${i}`}));
 if(duplicate)fields.push({...fields[1],selector:'#other'});
 let focusedSelector=null,clock=0;
 const context={element,Date:{now:()=>clock},visualPanel:root,visualButton:{disabled:false},fieldVisionButton:{disabled:false,click(){}},fieldFocusSelect:{value:'',options:fields.map(field=>({value:field.selector}))},result:{session_id:'fixture',fields},fillInProgress:false,fillButton:null,fillCompleted:true,refresh:async()=>{},recoverFormVision(){},setTimeout:fn=>fn(),document:{createElement:tag=>element(tag)},sessionStorage:{getItem:key=>storage.get(key)||null,setItem:(key,v)=>storage.set(key,v),removeItem:key=>storage.delete(key)},api:async(path,body)=>{
  calls.push([path,body]);
  if(path.endsWith('visual-focus')){focusedSelector=body.selector;return {value_retained:retained,unverified_count:0,failed_fields:[],record_saved:focusSaved,failed_attachments:0,unverified_attachments:0};}
  if(path.endsWith('visual-review-start'))return {task_id:body.target_selector||'full',segments:1,segment:0,half:null,target_selector:body.target_selector,coverage_complete:coverage,page_signature:'fictional',screenshot:'fictional',record_saved:true};
  if(path.endsWith('visual-review-status')){
   const full=body.task_id==='full',i=full?0:Number(focusedSelector.slice(2)),items=full?expected:[expected[i]];
   if(expire&&!full)clock=120001;
   if(cancel&&!full)walk(root).find(node=>node.text==='Stop remaining close-ups').listeners.click();
   if(detach&&!full)root.isConnected=false;
   return {status:running?'running':'complete',visual_freshness:full?fresh:focusFresh,record_saved:statusSaved,freshness_record_saved:!alreadySaved,freshness_record_status:alreadySaved?'already_saved':'saved',visual_review:{comparisons:items,observations:items.filter((_,index)=>!unobserved||!full||index!==0).map((item,index)=>({label:item.label,visible_value:full&&index>0||!full&&badFocused?'misread':item.expected}))}};
  }
  throw Error('Unexpected endpoint');
 }};
 vm.createContext(context);vm.runInContext(helpers+'\n'+focus+'\n'+visual,context);
 const outcome=await context.inspectVisual();return {calls,root,outcome,context,storage};
}
(async()=>{
 let x=await run();assert.equal(x.calls.length,11);assert.deepEqual(x.calls.filter(c=>c[0].endsWith('visual-focus')).map(c=>c[1].selector),['#f1','#f2','#f3']);assert.equal(x.outcome.followup.resolved,3);assert.equal(x.outcome.followup.remaining,0);assert.ok(walk(x.root).some(n=>n.text.includes('earlier page captures historical')));
 x=await run({mismatches:5});assert.equal(x.outcome.followup.checked,3);assert.equal(x.outcome.followup.remaining,2);assert.ok(walk(x.root).some(n=>n.text.includes('limit was reached')));
 for(const options of [{coverage:false},{fresh:'changed'},{statusSaved:false},{unobserved:true},{duplicate:true},{domUnconfirmed:true}]){x=await run(options);assert.equal(x.calls.length,2,JSON.stringify(options));assert.equal(x.outcome.followup,undefined);}
 for(const options of [{retained:false},{focusSaved:false}]){x=await run(options);assert.equal(x.calls.length,3);assert.equal(x.outcome.followup.stopped,true);assert.equal(x.outcome.followup.remaining,2);}
 x=await run({focusFresh:'changed'});assert.equal(x.calls.length,5);assert.equal(x.outcome.followup.checked,0);assert.equal(x.outcome.followup.attempted,1);assert.equal(x.outcome.followup.stopped,true);
 x=await run({cancel:true});assert.equal(x.calls.length,5);assert.equal(x.outcome.followup.checked,1);assert.equal(x.outcome.followup.remaining,2);assert.equal(x.outcome.followup.stopped,true);
 x=await run({detach:true});assert.equal(x.calls.length,5);assert.equal(x.outcome.followup.checked,1);
 x=await run({alreadySaved:true});assert.equal(x.outcome.followup.resolved,3);assert.ok(!walk(x.root).some(n=>n.text.includes('capture check was not saved')));
 x=await run({expire:true});assert.equal(x.outcome.followup.checked,1);assert.equal(x.outcome.followup.remaining,2);
 x=await run({running:true});assert.equal(x.calls.length,241);assert.equal(x.outcome.status,'incomplete');assert.equal(x.storage.size,1);assert.equal(x.context.visualButton.disabled,false);
 x=await run({badFocused:true});assert.equal(x.outcome.followup.checked,3);assert.equal(x.outcome.followup.resolved,0);assert.ok(x.outcome.followup.checks.every(check=>check.result.comparisons[0].status==='transcription differs from reviewed answer'));
 assert.ok(x.calls.every(call=>['/api/form/visual-focus','/api/form/visual-review-start','/api/form/visual-review-status'].includes(call[0])));
 assert.equal(x.context.visualButton.disabled,false);assert.equal(x.context.fieldVisionButton.disabled,false);assert.equal(x.outcome.comparisons[1].status,'transcription differs from reviewed answer');
 console.log('Bounded multi-field visual follow-up, explicit warnings, archive, cancellation and stale-value checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1});
