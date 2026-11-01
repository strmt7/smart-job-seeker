const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),test=require('node:test');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8'),helper=source.split('\n').find(line=>line.startsWith('function reviewedDimensionText(')),ctx={};vm.runInNewContext(helper,ctx);
test('reviewed dimensions distinguish job and preference evidence without claiming completeness',()=>{
 const text=ctx.reviewedDimensionText({dimensions:{job_requirements:{criteria:2,score:88,score_range:{lower:75,upper:100}},user_preferences:{criteria:1,score:0,score_range:{lower:0,upper:0}}}});
 assert.match(text,/Job criteria: 88%/);assert.match(text,/Your preferences: 0%/);assert.match(text,/not omitted requirements/);
 assert.equal(ctx.reviewedDimensionText({}), '');assert.equal(ctx.reviewedDimensionText({dimensions:{job_requirements:{criteria:1,score:90,score_range:{lower:0,upper:20}}}}),'');
});
