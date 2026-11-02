const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const helper=source.split('\n').find(line=>line.startsWith('function showReviewOnlyRecovery('));
assert.ok(helper);
function node(tag,text=''){return {tag,textContent:text,children:[],listeners:{},append(...items){this.children.push(...items);},addEventListener(event,fn){this.listeners[event]=fn;}};}
const calls=[],context={element:node,prepareObservedRole:(...args)=>calls.push(args)};
vm.runInNewContext(helper,context);
const card=node('article');context.showReviewOnlyRecovery(card,{url:'https://example.org/listing'},{review_only:true,job:{title:'Fictional Engineer',employer:'Example'}});
assert.match(card.children[0].textContent,/unverified/);
card.children[1].listeners.click();
assert.deepEqual(calls[0],['Fictional Engineer','https://example.org/listing','Example']);
const redirected=node('article');context.showReviewOnlyRecovery(redirected,{url:'https://example.org/listing'},{review_only:true,review_reason:'redirect',job:{title:'Different role',employer:'Example'}});
assert.match(redirected.children[0].textContent,/different role/);
const ordinary=node('article');context.showReviewOnlyRecovery(ordinary,{url:'https://example.org/listing'},{review_only:false,job:{title:'Role',employer:'Example'}});assert.equal(ordinary.children.length,0);
console.log('Review-only recovery checks passed.');
