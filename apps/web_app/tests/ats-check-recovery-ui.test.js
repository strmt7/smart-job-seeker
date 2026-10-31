const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js','utf8').split('\n');
const get = prefix => source.find(line=>line.startsWith(prefix));
function node(tag,text=''){return {tag,textContent:text,children:[],value:'',append(...items){this.children.push(...items);},replaceChildren(...items){this.children=items;},addEventListener(name,fn){this[name]=fn;},setAttribute(){},scrollIntoView(){},get selectedOptions(){return [{textContent:'Fictional role'}];}};}
(async()=>{
 let stored=null,active=null,starts=0,statuses=0,loseStart=true,loseStatus=true;
 const panel=node('panel'), doc={id:'doc-a',name:'Fictional.pdf',job_id:'job-a'};
 const sessionStorage={getItem:()=>stored,setItem:(_,v)=>stored=v,removeItem:()=>stored=null};
 const api=async(path)=>{
  if(path.endsWith('ats-check-active'))return {active};
  if(path.endsWith('ats-check-start')){starts++;active={task_id:'task-a',document_id:'doc-a',job_id:'job-a'};if(loseStart){loseStart=false;throw Error('Disconnected');}return {task_id:'task-a'};}
  if(path.endsWith('ats-check-status')){statuses++;if(loseStatus){loseStatus=false;throw Error('Disconnected');}active=null;return {status:'complete',report:{extracted_words:1,extracted_characters:4,limitations:'Fixture',structure_notes:[],reading_order_preview:[],terms:[],terms_truncated:false}};}
  throw Error(path);
 };
 const ctx=vm.createContext({sessionStorage,api,state:{jobs:[{job:{id:'job-a',title:'Fictional',employer:'Example'}}]},setWorkspaceView(){},notify(){},element:node,document:{querySelector:()=>panel,createElement:node},Option:function(text,value){this.textContent=text;this.value=value;},setTimeout:fn=>fn()});
 vm.runInContext(get('function pendingAtsCheck()'),ctx);
 vm.runInContext(get('function showAtsCheck(doc)'),ctx);
 ctx.showAtsCheck(doc);
 const button=panel.children.find(x=>x.textContent==='Check CV against job');
 await button.click();assert.equal(starts,1);assert.equal(stored,null);
 await button.click();assert.equal(starts,1);assert.ok(stored);assert.equal(statuses,1);
 await button.click();assert.equal(starts,1);assert.equal(statuses,2);assert.equal(stored,null);
 assert.match(panel.children.find(x=>x.tag==='div').children.map(x=>x.textContent).join(' '),/Extracted 1 words/);
 console.log('CV check recovery UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
