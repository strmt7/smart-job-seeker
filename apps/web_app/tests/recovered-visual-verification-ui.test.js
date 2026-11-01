const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),test=require('node:test');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8'),line=source.split('\n').find(s=>s.startsWith('function recoveredVisualComparisonLabel(')),ctx={};vm.runInNewContext(line,ctx);
test('recovered visual results distinguish raw transcription from live-value verification',()=>{
 assert.match(ctx.recoveredVisualComparisonLabel({status:'unobserved',dom_status:'unconfirmed'}),/not confirmed/);
 assert.match(ctx.recoveredVisualComparisonLabel({status:'matches_transcription',dom_status:'unconfirmed'}),/not confirmed/);
 assert.match(ctx.recoveredVisualComparisonLabel({status:'matches_transcription'}),/live field value was not confirmed/);
 assert.match(ctx.recoveredVisualComparisonLabel({status:'matches_transcription',dom_status:'matches'}),/matched at the time/);
 assert.match(ctx.recoveredVisualComparisonLabel({status:'differs_from_transcription'}),/differed/);
});
