const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),test=require('node:test');
const line=fs.readFileSync('apps/web_app/web/app.js','utf8').split('\n').find(s=>s.startsWith('function fitInputCoverageNotice('));const ctx={};vm.runInNewContext(line,ctx);
test('input coverage explains sampled omissions and rejects invalid accounting',()=>{
 const c={listing_used_chars:7900,listing_total_chars:12000,instructions_used_chars:0,instructions_total_chars:0,listing_sampling:'evenly_spaced'};
 assert.match(ctx.fitInputCoverageNotice(c),/spaced passages/);assert.match(ctx.fitInputCoverageNotice(c),/range does not account/);
 assert.equal(ctx.fitInputCoverageNotice({...c,listing_used_chars:12000}), '');
 assert.match(ctx.fitInputCoverageNotice({...c,listing_used_chars:12001}),/inconsistent/);
 assert.match(ctx.fitInputCoverageNotice({}),/unavailable/);
});
