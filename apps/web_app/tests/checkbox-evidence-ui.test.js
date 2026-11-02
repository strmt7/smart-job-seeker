const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const line=source.split('\n').find(x=>x.startsWith('function checkboxEvidenceControl('));
assert.ok(line);
const calls=[],saved=new Map();
function element(tag,text=''){return {tag,text,children:[],listeners:{},append(...nodes){this.children.push(...nodes)},replaceChildren(...nodes){this.children=nodes},addEventListener(name,fn){this.listeners[name]=fn}};}
const context={element,showAnswerEvidence(){},setTimeout:fn=>fn(),sessionStorage:{getItem:k=>saved.get(k)||null,setItem:(k,v)=>saved.set(k,v),removeItem:k=>saved.delete(k)},api:async(path,body)=>{calls.push({path,body});if(path.endsWith('/answer-start'))return {task_id:'task'};if(path.endsWith('/answer-status'))return {status:'complete',answer:{text:'Relevant experience requires review.',sources:['Fictional qualification']}};throw Error('Unexpected operation');}};
const create=vm.runInNewContext(`(${line})`,context);
(async()=>{const box=create('session',{selector:'#skill',label:'Experience — Rust'}),button=box.children[0],output=box.children[1];await button.listeners.click();assert.deepEqual(calls.map(x=>x.path),['/api/form/answer-start','/api/form/answer-status']);assert.match(output.children.map(x=>x.text).join(' '),/does not select or fill the checkbox/);assert.match(output.children.map(x=>x.text).join(' '),/Fictional qualification/);assert.equal(saved.size,0);console.log('Checkbox evidence UI checks passed.');})().catch(e=>{console.error(e);process.exitCode=1});
