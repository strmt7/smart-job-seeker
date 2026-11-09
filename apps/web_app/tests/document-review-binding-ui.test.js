const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const line = source.split('\n').find(value => value.startsWith('function bindDocumentReview('));
const nodes = Object.fromEntries(['#document-preview', '#pdf-preview'].map(id => [id, {
  dataset: { documentId: 'first' }, content: 'First document',
  replaceChildren() { this.content = ''; },
}]));
const bind = vm.runInNewContext(`(${line})`, {
  document: { querySelector: selector => nodes[selector] },
});
bind('first');
assert.equal(nodes['#pdf-preview'].content, 'First document');
bind('second');
for (const node of Object.values(nodes)) {
  assert.equal(node.content, '');
  assert.equal(node.dataset.documentId, 'second');
}
nodes['#document-preview'].content = 'Second document';
bind('second');
assert.equal(nodes['#document-preview'].content, 'Second document');
assert.ok(source.includes('bindDocumentReview(documentResult.id)'));
assert.ok(source.includes('bindDocumentReview(doc.id)'));
console.log('Document review panels preserve matching content and clear other documents.');
