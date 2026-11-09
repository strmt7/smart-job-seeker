const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const lines=fs.readFileSync('apps/web_app/web/app.js','utf8').split('\n');
function element(tag,text=''){return {tag,textContent:text,children:[],value:'',append(...nodes){this.children.push(...nodes);},replaceChildren(...nodes){this.children=nodes;},setAttribute(){},addEventListener(name,fn){this[name]=fn;}};}
const calls=[];const ctx=vm.createContext({element,documentJobCheckGeneration:1,Option:function(text,value){this.textContent=text;this.value=value;},document:{createElement:element},api:(path,body)=>new Promise(resolve=>calls.push({path,body,resolve}))});
for(const name of ['renderSavedDocumentReview','appendSavedDocumentReviews'])vm.runInContext(lines.find(line=>line.startsWith(`function ${name}(`)),ctx);
const digest='a'.repeat(64),attempt='b'.repeat(32),output=element('div');
ctx.appendSavedDocumentReviews(output,{document_sha256:digest,visual:{references:[{attempt_id:attempt,context_current:false,status:'complete'}]}},{id:'pdf'},'job',1);
const details=output.children[0],button=details.children.find(n=>n.tag==='button'),findings=details.children.find(n=>n.tag==='div');
const saved=()=>({document_id:'pdf',job_id:'job',attempt_id:attempt,document_sha256:digest,requires_user_review:true,context_current:false,status:'complete',report:{pages:1,pages_inspected:1,complete:true,findings:[{page:1,vision_status:'complete',text_geometry:{status:'checked',overlapping_pairs:1,clipped_chars:2},issues:[{category:'layout',description:'Fictional clipped heading'}]}]}});
(async()=>{
 const first=button.click();assert.equal(calls[0].path,'/api/documents/job-check-review');assert.equal(calls[0].body.document_id,'pdf');calls[0].resolve(saved());await first;
 let text=findings.children.map(n=>n.textContent).join(' ');assert.match(text,/context has changed/);assert.match(text,/Fictional clipped heading/);assert.match(text,/1 overlapping pairs; 2 clipped characters/);
 const wrong=button.click();calls[1].resolve({...saved(),document_sha256:'c'.repeat(64)});await wrong;assert.match(findings.children[0].textContent,/does not match/);
 const stale=button.click();ctx.documentJobCheckGeneration=2;calls[2].resolve(saved());await stale;assert.doesNotMatch(findings.children.map(n=>n.textContent).join(' '),/Fictional clipped heading/);
 console.log('Saved document findings recheck snapshot identity and preserve stale context.');
})().catch(error=>{console.error(error);process.exitCode=1;});
