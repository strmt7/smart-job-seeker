const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const html=fs.readFileSync('apps/web_app/web/index.html','utf8');
assert.match(html,/name="search_remote_only"/);assert.match(html,/name="search_locations"/);
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const line=source.split('\n').find(line=>line.startsWith('function renderProfileSummary('));
function node(tag,text=''){return {tag,text,childNodes:[],append(...items){this.childNodes.push(...items)},replaceChildren(){this.childNodes=[]}}}
const profile=node('div'),preferences=node('div');const document={querySelector:s=>s==='#profile-summary'?profile:preferences};
const render=vm.runInNewContext(`(${line})`,{element:node,document});
render({search_remote_only:'on',search_locations:'Berlin\nParis'});
assert.ok(preferences.childNodes.some(item=>item.text==='Remote roles'));
assert.ok(preferences.childNodes.some(item=>item.text==='Berlin\nParis'));
assert.match(source,/Outside search criteria:/);assert.match(source,/Search criterion needs review:/);
console.log('Search filter UI checks passed.');
