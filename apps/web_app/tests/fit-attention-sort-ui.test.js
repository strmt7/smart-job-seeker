const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const line = name => source.split('\n').find(x => x.startsWith(`function ${name}(`) || x.startsWith(`async function ${name}(`));
const constraintReviewLabel = vm.runInNewContext('('+line('constraintReviewLabel')+')');
assert.equal(constraintReviewLabel({unknown_constraints:['salary: not stated']}), 'Search criteria need review');
assert.equal(constraintReviewLabel({unknown_constraints:[' ', null]}), null);
assert.equal(constraintReviewLabel({unknown_constraints:[]}), null);
assert.equal(constraintReviewLabel({}), null);
const badgeStart = source.indexOf("side.append(element('span',job.state.replaceAll('_',' '),'job-state'));");
const badgeEnd = source.indexOf("card.addEventListener('click'", badgeStart);
assert.ok(badgeStart >= 0 && badgeEnd > badgeStart);
for (const [unknown, shown] of [[['salary: not stated'], true], [[], false]]) {
  const side = {children:[], append(node){this.children.push(node);}};
  vm.runInNewContext(source.slice(badgeStart,badgeEnd), {
    side, row:{unknown_constraints:unknown}, job:{state:'discovered'}, needsReview:false,
    constraintReviewLabel, element:(_tag,text,cls)=>({text,cls})
  });
  assert.equal(side.children.some(node=>node.text==='Search criteria need review'), shown);
}
const rows = ['clear', 'unknown', 'conflict', 'missing', 'excluded'].map(id => ({job:{id,title:id,employer:'E',location:''}, ...(id === 'excluded' ? {excluded_reasons:['constraint']} : {})}));
const nodes = {'#filter':{value:''},'#review-only':{checked:false},'#sort-reviewed':{},'#sort-attention':{},'#sort-default':{},'#sort-reviewed-status':{}};
const item = id => id === 'missing' ? {id,status:'missing'} : {id,status:'current',requirement_coverage:id === 'conflict' || id === 'excluded' ? 'possible_conflict' : id === 'unknown' ? 'needs_review' : 'supported', preference_coverage:'not_requested'};
const context = {state:{jobs:rows,reviews:[]},reviewedOrder:null,reviewedOrderGeneration:0,attentionSnapshot:false,jobPage:1,document:{querySelector:key=>nodes[key]},renderJobs(){},api:async (_,body)=>({results:body.ids.map(item)})};
for (const name of ['filteredJobRows','orderByReviewedFit','resetReviewedOrder','reviewedOrderChanged','fitAttentionPriority','sortByFitAttention']) context[name] = vm.runInNewContext('('+line(name)+')',context);
(async()=>{
  assert.equal(context.fitAttentionPriority({status:'current',requirement_coverage:'none_identified',preference_coverage:'not_requested'}),1);
  assert.equal(context.fitAttentionPriority({status:'current',requirement_coverage:'incomplete',preference_coverage:'not_requested'}),1);
  assert.equal(context.fitAttentionPriority({status:'current',requirement_coverage:'supported',preference_coverage:'possible_conflict'}),3);
  await context.sortByFitAttention();
  assert.equal(context.orderByReviewedFit(rows,context.reviewedOrder).map(row=>row.job.id).join(','),'conflict,unknown,missing,clear,excluded');
  assert.match(nodes['#sort-reviewed-status'].textContent,/possible conflicts: 2; unknown required criteria or preferences: 1; missing or incomplete assessments: 1/);
  assert.equal(context.jobPage,0);
  context.reviewedOrderChanged();
  assert.match(nodes['#sort-reviewed-status'].textContent,/sort again/);
  context.resetReviewedOrder();
  let release; context.api=()=>new Promise(resolve=>{release=resolve});
  const pending=context.sortByFitAttention();
  context.resetReviewedOrder();
  release({results:rows.map(row=>item(row.job.id))});
  await pending;
  assert.equal(context.reviewedOrder,null);
  assert.equal(nodes['#sort-reviewed-status'].textContent,'');
  context.api=async()=>({results:[]});
  await context.sortByFitAttention();
  assert.match(nodes['#sort-reviewed-status'].textContent,/incomplete/);
  console.log('Fit review prioritization checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
