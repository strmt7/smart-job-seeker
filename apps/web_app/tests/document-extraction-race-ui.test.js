const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf("extract.addEventListener('click',async()=>{"),end=source.indexOf('item.append(identity,extract,button)',start);
assert.ok(start>=0&&end>start);
let generation=0,shown=[],messages=[],resolveStatus;
const status=new Promise(resolve=>resolveStatus=resolve),handlers=[];
function install(doc,api){const extract={disabled:false,addEventListener:(_name,fn)=>handlers.push(fn)};const context={extract,doc,api,followDocumentExtraction:async task=>api('/api/documents/extract-status',{task_id:task}),notify:message=>messages.push(message),showDocument:value=>shown.push(value),setTimeout:fn=>fn(),get documentReviewGeneration(){return generation},beginDocumentReview:()=>++generation};vm.runInNewContext(source.slice(start,end),context);}
install({id:'first'},async path=>path.endsWith('start')?{task_id:'one',id:'first',name:'first.txt'}:status);
install({id:'second'},async()=>{throw Error('Extraction awaiting collection')});
(async()=>{const first=handlers[0]();await new Promise(resolve=>setImmediate(resolve));await handlers[1]();assert.match(messages[0],/awaiting collection/);resolveStatus({status:'complete',text:'Fictional CV'});await first;assert.equal(shown.length,1);assert.equal(shown[0].id,'first');console.log('Concurrent document-review check passed.');})().catch(e=>{console.error(e);process.exitCode=1});
