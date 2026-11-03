const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const start=source.indexOf('function sameCandidateSelection('),end=source.indexOf('\nasync function saveCandidateSelection(',start);
assert.ok(start>=0 && end>start);
const items=[{url:'https://example.org/job',title:'Fictional'}];
const compare=vm.runInNewContext(`${source.slice(start,end)}; sameCandidateSelection`);
const attribution={engines:['one'],published_date:'2026-10-01',alternative_published_dates:[],engines_limited:false,dates_limited:false};
assert.equal(compare([{...items[0],search_context:attribution}],[{...items[0],search_context:{...attribution,engines:['two']}}]),false);
assert.equal(compare([{...items[0],search_context:attribution}],[{...items[0],search_context:{...attribution,alternative_published_dates:['2026-10-02']}}]),false);
assert.equal(compare([{...items[0],search_context:attribution}],[{...items[0],search_context:{...attribution,coverage_limited:false}}]),true);
assert.equal(compare([{...items[0],search_context:attribution}],[{...items[0],search_context:{...attribution,coverage_limited:true}}]),false);
assert.equal(compare([{...items[0],search_context:attribution}],[{...items[0],search_context:{...attribution,provider_sponsored_marker:true}}]),false);
assert.equal(compare([{...items[0],search_context:attribution}],[{...items[0],search_context:{...attribution,provider_sponsored_marker:false}}]),true);
assert.equal(compare([{...items[0],search_context:{engines:[],published_date:'',alternative_published_dates:[],engines_limited:false,dates_limited:false}}],items),true);
assert.equal(compare([{...items[0],search_context:{engines:[]}}],items),false);
async function run(mode){let calls=[];const context={api:async(_path,body)=>{calls.push(body.action);if(body.action==='load'){if(mode==='committed')return {revision:5,items:[{title:'Fictional',url:'https://example.org/job'}]};if(mode==='conflict')return {revision:5,items:[]};return {revision:4,items:[]};}if(mode==='retry' && calls.filter(x=>x==='save').length===2)return {revision:5};throw Error('Lost response');}};const save=vm.runInNewContext(`${source.slice(start,end)}; persistCandidateSnapshot`,context);return {save,calls};}
(async()=>{let t=await run('committed');assert.equal(await t.save(4,items),5);assert.deepEqual(t.calls,['save','load']);t=await run('committed');await assert.rejects(()=>t.save(4,[{...items[0],search_context:attribution}]),/another window/);assert.deepEqual(t.calls,['save','load']);t=await run('retry');assert.equal(await t.save(4,items),5);assert.deepEqual(t.calls,['save','load','save']);t=await run('conflict');await assert.rejects(()=>t.save(4,items),/another window/);assert.deepEqual(t.calls,['save','load']);t=await run('failed');await assert.rejects(()=>t.save(4,items),/Lost response/);assert.deepEqual(t.calls,['save','load','save','load']);console.log('Candidate selection retry checks passed.');})().catch(error=>{console.error(error);process.exitCode=1;});
