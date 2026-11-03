const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js','utf8');
const line = name => source.split('\n').find(value=>value.startsWith(`function ${name}(`)||value.startsWith(`async function ${name}(`));
function node(tag,text=''){return {tag,textContent:text,children:[],listeners:{},setAttribute(){},append(...children){this.children.push(...children)},replaceChildren(...children){this.children=children},addEventListener(name,fn){this.listeners[name]=fn},remove(){}}}
function fixture(missingOption=false){
  const fields={first_name:{value:'My edit',labels:[{textContent:'First name'}]},city:{value:'Old city',labels:[{textContent:'City'}]},cv_template_id:{tagName:'SELECT',value:'old',options:[{value:'old'},...(missingOption?[]:[{value:'new'}])],labels:[{textContent:'CV template'}]}};
  const form={elements:fields,children:[],querySelector(){return null;},append(item){this.children.push(item)}};
  const latest={profile_revision:'b'.repeat(64),profile:{first_name:'Other edit',city:'New city',cv_template_id:'new'}};
  const ctx={profileEditRevision:'a'.repeat(64),profileEditBaseline:{first_name:'Original',city:'Old city',cv_template_id:'old'},api:async()=>latest,element:node,FormData:function(){return Object.entries(fields).map(([key,field])=>[key,field.value]);}};
  vm.createContext(ctx);vm.runInContext(`${line('planProfileRebase')}\n${line('offerProfileRebase')}`,ctx);
  return {ctx,form,fields};
}
(async()=>{
  const x=fixture();assert.throws(()=>x.ctx.planProfileRebase({}, {}, []),/unavailable/);await x.ctx.offerProfileRebase(x.form);const box=x.form.children[0];box.children.find(item=>item.tag==='button').listeners.click();
  assert.equal(x.fields.first_name.value,'My edit');assert.equal(x.fields.city.value,'New city');assert.equal(x.fields.cv_template_id.value,'new');
  assert.equal(x.ctx.profileEditRevision,'b'.repeat(64));assert.match(box.children[0].textContent,/conflicting fields.*First name/);
  const y=fixture(true);await y.ctx.offerProfileRebase(y.form);const failure=y.form.children[0];failure.children.find(item=>item.tag==='button').listeners.click();
  assert.equal(y.fields.city.value,'Old city');assert.equal(y.ctx.profileEditRevision,'a'.repeat(64));assert.match(failure.children[0].textContent,/selection is unavailable/);
  console.log('Profile rebase checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1});
