const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const lines = fs.readFileSync('apps/web_app/web/app.js','utf8').split('\n');
const context = vm.createContext({});
for (const name of ['documentPreparationStatus','documentMatchesFilter']) {
  vm.runInContext(lines.find(line=>line.startsWith(`function ${name}(`)),context);
}
const pdf={id:'a',name:'Résumé.PDF'},docx={id:'b',name:'CV draft.docx'},zip={id:'c',name:'Certificates.zip'};
assert.equal(context.documentMatchesFilter(pdf,'résumé','pdf'),true);
assert.equal(context.documentMatchesFilter(docx,'CV','pdf'),false);
assert.equal(context.documentMatchesFilter(docx,' draft ','convertible'),true);
assert.equal(context.documentMatchesFilter(zip,'','convertible'),false);
assert.equal(context.documentMatchesFilter(pdf,'','templates',{cv_template_id:'a'}),true);
assert.equal(context.documentMatchesFilter(docx,'','templates',{cv_template_id:'a'}),false);
assert.equal(context.documentMatchesFilter(zip,'','all'),true);
assert.match(context.documentPreparationStatus(pdf),/review before attaching/);
assert.match(context.documentPreparationStatus(docx),/Convert to PDF/);
assert.match(context.documentPreparationStatus(zip),/unavailable/);
assert.doesNotMatch(context.documentPreparationStatus(pdf),/ready/i);
console.log('Document library filtering and format checks passed.');
