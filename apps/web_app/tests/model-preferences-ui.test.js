'use strict';
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
class Node {
  constructor(tag,text=''){Object.assign(this,{tag,textContent:text,children:[],listeners:{},attributes:{},value:'',disabled:false});}
  append(...nodes){this.children.push(...nodes);}
  setAttribute(key,value){this.attributes[key]=value;}
  getAttribute(key){return this.attributes[key];}
  addEventListener(type,handler){this.listeners[type]=handler;}
}
function find(root,predicate){if(predicate(root))return root;for(const child of root.children){const found=find(child,predicate);if(found)return found;}}
const settle=()=>new Promise(resolve=>setImmediate(resolve));
(async()=>{
  const window={},root=new Node('div'),buttons=[],calls=[];
  const element=(tag,text)=>new Node(tag,text);
  let value={revision:0,choice:null},reject=false;
  const api=async(route,body)=>{calls.push({route,body});if(!body)return value;if(reject)throw Error('Model settings changed in another window');assert.equal(body.revision,value.revision);value={revision:value.revision+1,choice:body.model?{name:body.model,digest:'a'.repeat(64),size_bytes:2*1073741824,directory:body.directory}:null};return value;};
  vm.runInNewContext(fs.readFileSync('apps/web_app/web/model-preferences.js','utf8'),{window,document:{querySelectorAll:()=>buttons}});
  window.WaypointModelPreferences.mount(root,{api,element});await settle();
  const clear=find(root,n=>n.textContent==='Clear selection'),refresh=find(root,n=>n.textContent==='Refresh selection'),folder=find(root,n=>n.tag==='input');
  assert.equal(clear.disabled,true);
  const choose=window.WaypointModelPreferences.forModel({name:'fictional:vision'},true,element);buttons.push(choose);
  folder.value='C:/fictional/models';folder.listeners.input();
  await choose.listeners.click();
  assert.equal(value.choice.name,'fictional:vision');assert.equal(value.choice.directory,'C:/fictional/models');assert.equal(clear.disabled,false);
  assert(find(root,n=>n.textContent.includes('Bound to this installed model version')));
  reject=true;await choose.listeners.click();assert(find(root,n=>n.textContent.includes('Refresh the selection before retrying')));assert.equal(value.revision,1);
  reject=false;await refresh.listeners.click();await clear.listeners.click();assert.equal(value.choice,null);assert.equal(value.revision,2);assert.equal(folder.value,'');
  const unsafe=window.WaypointModelPreferences.forModel({name:'remote'},false,element);assert.equal(unsafe.disabled,true);
  assert.equal(calls.filter(c=>c.body).length,3);
  const html=fs.readFileSync('apps/web_app/web/index.html','utf8');assert(html.indexOf('/model-preferences.js')<html.indexOf('/app.js'));
  console.log('Model preference revision, conflict, clear, path and disabled-state checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
