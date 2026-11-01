const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf('function loadFitBadges(');
const end = source.indexOf('\nfunction matchDetails(', start);
assert.ok(start >= 0 && end > start);
const fn = source.slice(start, end);
function badge(updated = false) { return {dataset: updated ? {updated:'1'} : {}, textContent:'', hidden:true}; }
async function check(api, badges) {
 const context = {window:{}, fitRenderGeneration:1, api, showFitBadge:(b,text)=>{b.textContent=text;b.hidden=false;}, fitBadgeLabel:()=> 'score', fitBadgeWithReview:value=>value};
 vm.runInNewContext(fn, context);
 context.loadFitBadges(badges.map((b,i)=>({id:`job-${i}`,badge:b,card:{}})),1);
 await new Promise(resolve=>setImmediate(resolve));
}
(async()=>{
 let a=badge(),b=badge(),c=badge();
 await check(async()=>({results:[{id:'job-0',status:'unavailable'},{id:'job-1',status:'missing'}]}),[a,b,c]);
 assert.equal(a.textContent,'AI estimate unavailable');
 assert.equal(b.hidden,true);
 assert.equal(c.textContent,'AI estimate unavailable');
 a=badge();b=badge(true);
 await check(async()=>{throw new Error('offline')},[a,b]);
 assert.equal(a.textContent,'AI estimate unavailable');
 assert.equal(b.hidden,true);
 console.log('Fit batch failure UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1});
