const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf('function updateCandidateSelection()'),end=source.indexOf('\nfunction ',start+1);
assert.ok(start>=0 && end>start);
function node(tag,text='',className=''){return {tag,text,className,children:[],listeners:{},disabled:false,append(...items){this.children.push(...items);},replaceChildren(){this.children=[];},setAttribute(name,value){this[name]=value;},addEventListener(name,cb){this.listeners[name]=cb;}};}
const list=node('ul'),count=node('span'),startButton=node('button');
const checkbox={checked:true,getAttribute:()=> 'https://example.org/job'};
const selected=new Map([['https://example.org/job',{url:'https://example.org/job',title:'Fictional engineer'}]]);let saves=0;
const context={selectedCandidates:selected,candidateSelectionReady:true,candidateSelectionCanSave:true,candidateSelectionSaving:false,candidateSelectionSaved:0,candidateSelectionRevision:0,candidateBatchRunning:false,candidateBatch:null,element:node,queueCandidateSelectionSave:()=>saves++,document:{querySelector:id=>({'#selected-candidates-list':list,'#candidate-selected-count':count,'#candidate-batch-start':startButton})[id],querySelectorAll:()=>[checkbox]}};
vm.runInNewContext(source.slice(start,end),context);
context.updateCandidateSelection();assert.equal(list.children.length,1);const row=list.children[0];assert.equal(row.children[0].children[0].text,'Fictional engineer');assert.equal(row.children[0].children[1].text,'https://example.org/job');assert.equal(row.children[1].disabled,false);
context.candidateBatch={};context.updateCandidateSelection();assert.equal(list.children[0].children[1].disabled,true);
context.candidateBatch=null;context.updateCandidateSelection();list.children[0].children[1].listeners.click();assert.equal(selected.size,0);assert.equal(saves,1);assert.equal(checkbox.checked,false);assert.equal(count.textContent,'0 listings selected');assert.equal(list.children[0].text,'No listings selected.');
assert.match(fs.readFileSync('apps/web_app/web/index.html','utf8'),/id="selected-candidates-list"/);
console.log('Selected listing review checks passed.');
