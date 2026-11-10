const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
class Node {
  constructor(tag,text='',style=''){Object.assign(this,{tag,textContent:text,style,children:[],handlers:{},attributes:{}});}
  append(...children){this.children.push(...children);}
  replaceChildren(...children){this.children=children;}
  setAttribute(key,value){this.attributes[key]=value;}
  addEventListener(type,fn){this.handlers[type]=fn;}
}
const window={};
vm.runInNewContext(fs.readFileSync('apps/web_app/web/source-copy.js','utf8'),{window});
const view=outline=>window.WaypointSourceCopies.outlineView(outline,(...args)=>new Node(...args));
const all=node=>[node,...node.children.flatMap(all)];
assert.match(view(null).textContent,/not assessed/);
assert.match(view({status:'unavailable'}).textContent,/could not be inspected/);
const controls=Array.from({length:30},(_,i)=>({role:'checkbox',label:i===0?'<img src=private>':'Yes',context:[i%2?'Relocation':'Work authorization'],required:null,disabled:i===0?true:null}));
const panel=view({status:'partial',controls});
const list=panel.children.find(n=>n.tag==='ol'),more=panel.children.find(n=>n.tag==='button');
assert.equal(list.children.length,12);
assert.equal(more.type,'button');
assert.equal(list.children[0].children.find(n=>n.tag==='strong').textContent,'<img src=private>');
assert(!all(panel).some(n=>n.tag==='img'||n.tag==='input'||n.tag==='a'));
assert(all(panel).some(n=>n.textContent==='Work authorization'));
assert(all(panel).some(n=>n.textContent==='Relocation'));
assert(all(panel).some(n=>/Requirement not exposed/.test(n.textContent)));
assert(!all(panel).some(n=>/Optional/.test(n.textContent)));
more.handlers.click();assert.equal(list.children.length,30);assert.equal(more.attributes['aria-expanded'],'true');
more.handlers.click();assert.equal(list.children.length,12);assert.equal(more.attributes['aria-expanded'],'false');
assert.match(view({status:'observed',controls:[]}).children[2].textContent,/No supported controls/);
assert.match(view({status:'observed',controls:[null]}).textContent,/could not be inspected/);
const link=view({status:'observed',controls:[{role:'link',label:'Application information',context:[],required:null,disabled:null}]});
assert(all(link).some(n=>n.textContent==='Link'));
assert(!all(link).some(n=>/Requirement not exposed|Optional|State not exposed/.test(n.textContent)));
console.log('Source form outline preserves unknowns, context and text-only labels with bounded expansion.');
