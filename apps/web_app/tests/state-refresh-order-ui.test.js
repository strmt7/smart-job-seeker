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
const form = {elements:{cv_template_id:select,cover_template_id:select}};
const context = {
  stateRefreshIssued:0,stateRefreshApplied:0,state:{},jobObservationSnapshotAt:0,
  selectedJobs:new Set(),reviewSelectedJobs:new Set(),selectionReconciled:false,profileLoaded:true,
  api:()=>new Promise((resolve,reject)=>deferred.push({resolve,reject})),
  resetReviewedOrder(){},renderProfileSummary(){},reconcileJobSelection(){},updateReviewSelection(){},renderJobs(){},renderReviewQueue(){},scheduleAutomaticFit(){},
  element:node,Option:function(label,value){this.label=label;this.value=value;},
  document:{querySelector(selector){if(selector==='#profile-form')return form;if(!nodes.has(selector))nodes.set(selector,node());return nodes.get(selector);}},
};
vm.createContext(context);
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
  console.log('State refresh ordering checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1});
