const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const line=source.split('\n').find(line=>line.startsWith('function showTemplateVision('));
assert.ok(line);
function node(tag,text='',className=''){return {tag,text,className,children:[],append(...items){this.children.push(...items)}}}
const render=vm.runInNewContext('('+line+')',{element:node,document:{createElement:node}});
const panel=node('div');
render(panel,{page:2,pages:3,uncertain:true,other_pages_not_transcribed:2,text:'Fictional text',image:'fixtureBase64'});
const details=panel.children[0],image=details.children.find(child=>child.tag==='img');
assert.equal(image.src,'data:image/png;base64,fixtureBase64');
assert.match(image.alt,/page 2/i);
assert.match(details.children[1].text,/uncertainty/);
assert.equal(details.children.at(-1).text,'Fictional text');
const older=node('div');render(older,{page:1,pages:1,text:'Legacy',other_pages_not_transcribed:0});
assert.match(older.children[0].children[2].text,/image is unavailable/);
console.log('Scanned template image review checks passed.');
