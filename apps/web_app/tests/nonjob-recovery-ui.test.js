const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const helper=source.split('\n').find(line=>line.startsWith('function showReviewOnlyRecovery('));
assert.ok(helper);
function node(tag,text=''){return {tag,textContent:text,children:[],listeners:{},append(...items){this.children.push(...items);},addEventListener(event,fn){this.listeners[event]=fn;}};}
const calls=[],context={element:node,URL,prepareObservedRole:(...args)=>calls.push(args)};
vm.runInNewContext(helper,context);
const card=node('article');context.showReviewOnlyRecovery(card,{url:'https://example.org/listing'},{review_only:true,job:{title:'Fictional Engineer',employer:'Example'}});
assert.match(card.children[0].textContent,/unverified/);
card.children[1].listeners.click();
assert.deepEqual(calls[0],['Fictional Engineer','https://example.org/listing','Example']);
const redirected=node('article');context.showReviewOnlyRecovery(redirected,{url:'https://example.org/listing'},{review_only:true,review_reason:'redirect',review_destination:'https://example.org/careers',job:{title:'Different role',employer:'Example'}});
assert.match(redirected.children[0].textContent,/different role/);
assert.equal(redirected.children[1].href,'https://example.org/careers');
redirected.children[2].listeners.click();
assert.deepEqual(calls[1],['','https://example.org/listing','']);
const external=node('article');context.showReviewOnlyRecovery(external,{url:'https://example.org/listing'},{review_only:true,review_reason:'redirect',review_destination:'https://evil.example/careers',job:{title:'Different role',employer:'Example'}});
assert.equal(external.children.filter(child=>child.tag==='a').length,0);
const ordinary=node('article');context.showReviewOnlyRecovery(ordinary,{url:'https://example.org/listing'},{review_only:false,job:{title:'Role',employer:'Example'}});assert.equal(ordinary.children.length,0);
console.log('Review-only recovery checks passed.');

const closed=node('article');context.showReviewOnlyRecovery(closed,{url:'https://example.org/listing'},{review_only:true,job:{title:'Fictional Engineer',employer:'Example',possible_closure_evidence:'This fictional vacancy has expired.'}});
assert.ok(closed.children.some(item=>/Possible closure wording.*expired.*Verify/.test(item.textContent)));
const failed=node('article');context.showReviewOnlyRecovery(failed,{url:'https://example.org/listing'},{review_only:true,job:{title:'Fictional Engineer',employer:'Example',extraction_warnings:['Local AI could not complete the closure check; verify availability on the original page.',null]}});
assert.ok(failed.children.some(item=>/could not complete the closure check/.test(item.textContent)));
assert.ok(failed.children.some(item=>/unverified/.test(item.textContent)));
