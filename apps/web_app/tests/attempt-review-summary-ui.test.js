const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf('function renderAttemptReviewSummary('),end=source.indexOf('function renderAttemptAnswers(',start);
assert.ok(start>=0&&end>start);
function element(tag,text='',className=''){return {tag,text,className,children:[],append(...items){this.children.push(...items);},replaceChildren(...items){this.children=items;}};}
const context={element};vm.runInNewContext(source.slice(start,end),context);
const panel=element('div');
const flatten=node=>[node.text,...node.children.flatMap(flatten)].join('\n');
const base={available:true,coverage:'complete',activity:[],other_events:0,omitted_activity:0,unreadable_events:0};
context.renderAttemptReviewSummary(panel,{...base,coverage:'partial',unreadable_events:2,other_events:3,omitted_activity:4,activity:[
 {title:'Field entry stopped',guidance:'Inspect before retrying',time:2,needs_review:true},
 {title:'<img src=x onerror=alert(1)>',guidance:'<script>fictional</script>',time:3,needs_review:false}]});
let text=flatten(panel);assert.match(text,/current form and submission status are not assessed/);assert.match(text,/Earlier issues may have been resolved/);
assert.match(text,/Only part/);assert.match(text,/2 entries/);assert.match(text,/3 other/);assert.match(text,/4 earlier/);
assert.equal(panel.children.at(-1).children[0].text,'Field entry stopped');
assert.equal(panel.children.at(-2).children[0].text,'<img src=x onerror=alert(1)>');
assert.ok(panel.children.every(node=>node.tag!=='script'&&node.tag!=='img'));
context.renderAttemptReviewSummary(panel,{...base,available:false,coverage:'unavailable'});assert.match(flatten(panel),/No activity log/);
context.renderAttemptReviewSummary(panel,base);assert.match(flatten(panel),/does not establish a successful attempt/);
for(const bad of [null,{...base,activity:Array(41).fill({})},{...base,coverage:'safe'},{...base,activity:[{title:'x',guidance:'y'}]}])assert.throws(()=>context.renderAttemptReviewSummary(panel,bad),/Invalid/);
(async()=>{let call;context.api=async(path,body)=>{call={path,body};return {...base,activity:[{title:'Record loaded',guidance:'Review it',needs_review:true}]};};
 await context.loadAttemptReviewSummary(panel,'job','attempt');assert.equal(call.path,'/api/attempts/review-summary');assert.equal(call.body.id,'job');assert.equal(call.body.attempt_id,'attempt');assert.match(flatten(panel),/Record loaded/);
 context.api=async()=>{throw new Error('offline');};await context.loadAttemptReviewSummary(panel,'job','attempt');assert.match(flatten(panel),/Could not load.*offline/);
 assert.ok(source.includes("element('button','View review summary')"));console.log('Saved review summary UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
