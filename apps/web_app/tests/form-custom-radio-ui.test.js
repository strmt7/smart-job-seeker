const assert=require('node:assert/strict');
const fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf(' const radioButton='),end=source.indexOf('\n const sessionEstimate=',start);
assert.ok(start>=0&&end>start);
class Node {constructor(tag,text=''){this.tag=tag;this.text=text;this.children=[];this.listeners={};}setAttribute(){} append(...items){this.children.push(...items);}replaceChildren(...items){this.children=items;}addEventListener(name,fn){this.listeners[name]=fn;}}
const element=(tag,text='')=>new Node(tag,text),panel=new Node('panel');
const api=async(path,body)=>{assert.equal(path,'/api/form/custom-radio-groups');assert.equal(body.session_id,'test');return {groups:[{label:'Work arrangement',options:[{label:'Remote',checked:true,disabled:false},{label:'On site',checked:null,disabled:true}],incomplete:true}],incomplete:true};};
const run=vm.runInNewContext(`(function(panel,result,element,api){${source.slice(start,end)};return {radioButton,radioPanel};})`);
(async()=>{const {radioButton,radioPanel}=run(panel,{session_id:'test'},element,api);await radioButton.listeners.click();assert.equal(radioPanel.children[0].children[0].text,'Work arrangement');assert.equal(radioPanel.children[0].children[1].children[0].text,'Remote — reported selected');assert.equal(radioPanel.children[0].children[1].children[1].text,'On site — selection unknown (disabled)');assert.match(radioPanel.children[0].children[2].text,/could not be inspected/);assert.match(radioPanel.children[1].text,/could not be inspected/);assert.match(radioPanel.children[2].text,/page-reported/);console.log('Custom radio review UI checks passed.');})().catch(error=>{console.error(error);process.exitCode=1;});
