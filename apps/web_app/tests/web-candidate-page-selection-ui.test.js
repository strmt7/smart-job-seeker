const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const candidate=source.split('\n').find(line=>line.startsWith('function webCandidate('));
const start=source.indexOf('function renderWebBatch('),end=source.indexOf('\nfunction webBatchSummary(',start);
assert.ok(candidate && start>=0 && end>start);
function node(tag,text=''){return {tag,textContent:text,children:[],listeners:{},append(...items){this.children.push(...items);},replaceChildren(...items){this.children=[...items];},addEventListener(event,fn){this.listeners[event]=fn;},setAttribute(){}};}
const panel=node('div'),selected=new Map(),notices=[];
const context={selectedCandidates:selected,webBatchPage:0,element:node,searchCoverage:()=>'',notify:text=>notices.push(text),updateCandidateSelection(){},document:{querySelector:()=>panel,createElement:tag=>node(tag),createTextNode:text=>node('text',text)}};
const render=vm.runInNewContext(`${candidate}\n${source.slice(start,end)}\nrenderWebBatch`,context);
const items=Array.from({length:120},(_,i)=>({url:`https://example.test/${i}`,title:`Role ${i}`,snippet:''}));
const batch={groups:[{query:'roles',page:1,data:{results:items}}]};
function select(){const controls=panel.children[0],button=controls.children.find(item=>item.textContent==='Select this page');button.listeners.click();}
render(batch);select();assert.equal(selected.size,40);
context.webBatchPage=1;render(batch);select();assert.equal(selected.size,80);
context.webBatchPage=2;render(batch);select();assert.equal(selected.size,100);assert.match(notices[0],/20 listings were not selected/);
const clear=panel.children[0].children.find(item=>item.textContent==='Clear selected listings');clear.listeners.click();assert.equal(selected.size,0);
assert.ok(source.includes('pending.items.length<=100'));
console.log('Web candidate page selection checks passed.');
