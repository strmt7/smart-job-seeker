const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const code=source.slice(source.indexOf('async function followConversion('),source.indexOf('function conversionError('));
const coverage=source.split('\n').find(line=>line.startsWith('function pdfExtractionCoverageNotice('));
async function run(status,scope,accepted_revision_view=false,pdf_coverage=null){
 let report,previewed=false;
 const resume={after:item=>{report=item;}};
 const context={conversionFollowing:false,updateConversionControl(){},sessionStorage:{removeItem(){}},setTimeout:fn=>fn(),refresh:async()=>{},notify(){},previewPdf:()=>{previewed=true;},
  api:async()=>({status:'complete',id:'fictional',name:'cv.pdf',text_check:{status,scope,accepted_revision_view,pdf_coverage}}),
  document:{querySelector:selector=>selector==='#resume-conversion'?resume:report},
  element:(_tag,text)=>({text,addEventListener(_event,fn){this.click=fn;},replaceChildren(...children){this.children=children;}})};
 await vm.runInNewContext(`${coverage}\n${code}; followConversion`,context)('task');
 report.children[1].click();
 assert.ok(previewed);
 return report.children[0].text;
}
(async()=>{
 assert.match(await run('matches'),/matches the source/);
 assert.match(await run('matches','docx_extract'),/matches the extracted DOCX text/);
 assert.match(await run('matches','docx_extract',true),/temporary accepted view.*original DOCX keeps its revision history/);
 assert.match(await run('differs'),/differs from the source/);
 assert.match(await run('unavailable'),/could not be compared/);
 assert.match(await run('not_checked'),/was not compared/);
 assert.match(await run('matches','docx_extract',false,{pages:1,pages_without_selectable_text:[],pages_with_images_and_text:[1],pages_with_unknown_image_inventory:[]}),/may contain text missing from the extraction/);
 console.log('Conversion result UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
