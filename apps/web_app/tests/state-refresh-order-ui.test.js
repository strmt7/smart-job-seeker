const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const line = source.split('\n').find(value => value.startsWith('async function refresh('));
assert.ok(line);
const deferred = [];
const select = {value:'',replaceChildren(){},append(){}};
const nodes = new Map();
function node() { return {textContent:'',replaceChildren(){},append(){}}; }
const form = {elements:{cv_template_id:select,cover_template_id:select,first_name:{value:''}}};
const context = {
  stateRefreshIssued:0,stateRefreshApplied:0,state:{},jobObservationSnapshotAt:0,profileEditBaseline:{first_name:'old draft base'},profileEditRevision:'a'.repeat(64),
  selectedJobs:new Set(),reviewSelectedJobs:new Set(),selectionReconciled:false,profileLoaded:true,
  api:()=>new Promise((resolve,reject)=>deferred.push({resolve,reject})),
  resetReviewedOrder(){},renderProfileSummary(){},reconcileJobSelection(){},updateReviewSelection(){},renderJobs(){},renderReviewQueue(){},renderDocuments(){},scheduleAutomaticFit(){},
  element:node,Option:function(label,value){this.label=label;this.value=value;},FormData:function(form){return [['first_name',form.elements.first_name.value]];},
  document:{querySelector(selector){if(selector==='#profile-form')return form;if(!nodes.has(selector))nodes.set(selector,node());return nodes.get(selector);}},
};
vm.createContext(context);
vm.runInContext(source.split('\n').find(value => value.startsWith('function setTemplateOptions(')),context);
vm.runInContext(line,context);
const data = marker => ({jobs:[],facts:[],reviews:[],documents:[],profile:{},sealing:marker});
(async()=>{
  const first=context.refresh(),second=context.refresh();
  deferred[1].resolve(data('newer'));
  await second;
  deferred[0].resolve(data('older'));
  await first;
  assert.equal(context.state.sealing,'newer');
  assert.equal(context.stateRefreshApplied,2);
  const third=context.refresh(),fourth=context.refresh();
  deferred[3].reject(new Error('offline'));
  await assert.rejects(fourth,/offline/);
  deferred[2].resolve(data('last successful'));
  await third;
  assert.equal(context.state.sealing,'last successful');
  assert.equal(context.profileEditBaseline.first_name,'old draft base');
  context.profileLoaded=false;
  const fifth=context.refresh();deferred[4].resolve({...data('initial'),profile:{first_name:'Fictional'},profile_revision:'c'.repeat(64)});await fifth;
  assert.equal(context.profileEditBaseline.first_name,'Fictional');assert.equal(context.profileEditRevision,'c'.repeat(64));
  const template = {value:'',options:[],replaceChildren(...options){this.options=options;},append(option){this.options.push(option);}};
  context.setTemplateOptions(template,'missing-id',[{id:'present',name:'CV.pdf'}]);
  assert.equal(template.value,'missing-id');
  assert.match(template.options.find(option=>option.value==='missing-id').label,/Unavailable document/);
  context.setTemplateOptions(template,'missing-id',[{id:'missing-id',name:'Restored.docx'}]);
  assert.equal(template.options.find(option=>option.value==='missing-id').label,'Restored.docx');
  context.setTemplateOptions(template,'',[]);
  assert.equal(template.value,'');assert.equal(template.options.length,1);
  console.log('State refresh ordering checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1});
