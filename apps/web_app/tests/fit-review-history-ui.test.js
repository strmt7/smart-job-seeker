const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8'),line=source.split('\n').find(x=>x.startsWith('function showFitReviewHistory('));
function node(tag,text=''){return {tag,textContent:text,children:[],append(...items){this.children.push(...items)}}}
const show=vm.runInNewContext('('+line+')',{element:node});const panel=node('div');
show(panel,[{model:'fixture',rubric:[{excerpt:'Rust'}],review:{score:75,decisions:[{importance:'required',status:'unknown'}]}}],2);
const details=panel.children[0];assert.match(details.children[0].textContent,/Previous reviewed scenarios/);assert.match(details.children[1].textContent,/historical/);assert.match(details.children[2].textContent,/2 older reviews/);assert.match(details.children.at(-1).textContent,/Rust · required · unknown/);
const empty=node('div');show(empty,[],0);assert.equal(empty.children.length,0);console.log('Historical match review UI checks passed.');
