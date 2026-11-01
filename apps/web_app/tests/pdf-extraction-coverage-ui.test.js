const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const line=source.split('\n').find(line=>line.startsWith('function pdfExtractionCoverageNotice('));
assert.ok(line);
const notice=vm.runInNewContext('('+line+')');
assert.match(notice({pages:3,pages_without_selectable_text:[2,3]}),/pages 2, 3 of 3.*blank or scanned/);
assert.equal(notice({pages:1,pages_without_selectable_text:[]}),null);
assert.match(notice({pages:2,pages_without_selectable_text:[2],structurally_blank_pages:[2]}),/page 2 has no page content or annotations and appears blank/);
assert.match(notice({pages:3,pages_without_selectable_text:[2,3],structurally_blank_pages:[2]}),/page 3 of 3.*page 2 has no page content/);
assert.equal(notice(null),null);
assert.match(source,/coverage:result\.coverage/);
console.log('PDF extraction coverage UI checks passed.');

assert.match(notice({pages:1,pages_without_selectable_text:[],pages_with_images_and_text:[1]}),/page 1 also contains images.*missing from the extraction/);
assert.match(notice({pages:1,pages_without_selectable_text:[],pages_with_unknown_image_inventory:[1]}),/could not be checked/);

assert.ok(source.includes("pdfExtractionCoverageNotice(draft.template_pdf_coverage)"));
assert.ok(source.includes("Review scanned template transcription"));
