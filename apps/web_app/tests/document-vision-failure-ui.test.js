const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const line = fs.readFileSync('apps/web_app/web/app.js', 'utf8').split('\n').find(value => value.startsWith('function showPageVisionFindings('));
const show = vm.runInNewContext(`(${line})`, {element: (tag,text) => ({tag,text})});
function text(page){const panel={children:[],append(...items){this.children.push(...items);}};show(panel,page);return panel.children.map(x=>x.text).join(' ');}
assert.match(text({vision_status:'failed',issues:[]}), /did not review/);
assert.doesNotMatch(text({vision_status:'failed',issues:[]}), /No issues/);
assert.match(text({vision_status:'complete',issues:[]}), /not a pass/);
assert.match(text({vision_status:'complete',issues:[{category:'layout',description:'Inspect'}]}), /layout: Inspect/);
console.log('Document vision failure UI checks passed.');
