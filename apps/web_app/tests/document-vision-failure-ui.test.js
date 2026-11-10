const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const line = fs.readFileSync('apps/web_app/web/app.js', 'utf8').split('\n').find(value => value.startsWith('function showPageVisionFindings('));
const show = vm.runInNewContext(`(${line})`, {element: (tag,text) => ({tag,text,children:[],append(...items){this.children.push(...items);}})});
function text(page){const panel={children:[],append(...items){this.children.push(...items);}};show(panel,page);function words(node){return [node.text||'',...(node.children||[]).map(words)].join(' ');}return panel.children.map(words).join(' ');}
assert.match(text({vision_status:'failed',issues:[]}), /did not review/);
assert.doesNotMatch(text({vision_status:'failed',issues:[]}), /No issues/);
assert.match(text({vision_status:'complete',issues:[]}), /not a pass/);
assert.match(text({vision_status:'complete',issues:[{category:'layout',description:'Inspect'}]}), /layout: Inspect/);
console.log('Document vision failure UI checks passed.');

const optional=text({vision_status:'complete',issues:[],suggestions:['Add an expected graduation date only if known.']});
assert.match(optional,/Optional suggestions/);assert.match(optional,/only if known/);assert.match(optional,/not a pass/);

const adjudicated=text({vision_status:'complete',issues:[],content_check:{status:'completed',examined:1,total:1,items:[{original:{description:'An observation affirming consistency.'},verdict:'not_issue',reason:'No contradiction was established.'}]}});
assert.match(adjudicated,/Content check: 1 of 1/);assert.match(adjudicated,/affirming consistency/);assert.match(adjudicated,/No defect found/);assert.doesNotMatch(adjudicated,/not_issue/);assert.match(adjudicated,/advisory/);
