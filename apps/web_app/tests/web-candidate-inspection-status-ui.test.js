const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const candidate = source.split('\n').find(line => line.startsWith('function webCandidate('));
const sponsored = source.split('\n').find(line => line.startsWith('function sponsoredMarkerMessage('));
assert.ok(sponsored);
const titleNotice = source.split('\n').find(line => line.startsWith('function searchTitleNotice('));
assert.ok(candidate);
assert.ok(titleNotice);
function node(tag, text = '') {
  return {tag, textContent:text, children:[], listeners:{}, attributes:{},
    append(...items){this.children.push(...items)}, addEventListener(event,fn){this.listeners[event]=fn},
    setAttribute(key,value){this.attributes[key]=value}};
}
const context = {candidateSelectionReady:true, selectedCandidates:new Map(), element:node,
  document:{createElement:node,createTextNode:text=>node('text',text)},
  api:async()=>{throw new Error('The page returned HTTP 404; manual review is required')},
  notify(){}, offerListingRecovery:async()=>{}, updateCandidateSelection(){}, queueCandidateSelectionSave(){},
  sessionStorage:{setItem(){}},followListingInspection:async()=>({saved:{existing:false},job:{title:'Different role'}}),showReviewOnlyRecovery(){},showOtherRoles(){},showReinspectionPreview(){}};
const render = vm.runInNewContext(`${titleNotice}\n${sponsored}\n${candidate}\nwebCandidate`, context);
(async()=>{
  const card = render({url:'https://example.org/role',title:'Role',snippet:''});
  const button = card.children.find(item=>item.textContent==='Inspect and add');
  await button.listeners.click();
  const status = card.children.find(item=>item.attributes?.role==='status');
  assert.match(status.textContent, /HTTP 404.*Review the original link/);
  assert.equal(button.disabled,false);
  context.api=async()=>({task_id:'fictional'});
  await button.listeners.click();
  assert.match(status.textContent, /Inspected page title: Different role.*different roles/);
  context.followListingInspection=async()=>({review_only:true,review_reason:'uncertain',saved:null,job:{title:'Role'}});
  await button.listeners.click();
  assert.match(status.textContent, /could not confirm this is a vacancy; nothing was saved/);
  assert.match(status.textContent, /Enter details manually/);
  context.followListingInspection=async()=>({review_only:true,review_reason:'not_vacancy',saved:null,job:{title:'Role',classification_sampled:true}});
  await button.listeners.click();
  assert.match(status.textContent, /used excerpts; omitted text/);
  console.log('Candidate inspection status UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1});
