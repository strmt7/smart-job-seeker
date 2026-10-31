const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const line = fs.readFileSync('apps/web_app/web/app.js', 'utf8').split('\n').find(value => value.startsWith('function showTextGeometryWarning('));
const show = vm.runInNewContext(`(${line})`, {element: (tag,text) => ({tag,text})});
function text(geometry){const panel={children:[],append(...items){this.children.push(...items);}};show(panel,geometry);return panel.children.map(x=>x.text).join(' ');}
assert.match(text({status:'checked',overlapping_pairs:2}), /may overlap/);
assert.equal(text({status:'checked',overlapping_pairs:0}), '');
assert.match(text({status:'no_selectable_text'}), /No selectable text/);
assert.match(text(undefined), /could not be checked/);
console.log('Document text geometry UI checks passed.');
