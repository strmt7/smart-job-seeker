const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf('function profileValuePicker(');
const end = source.indexOf('\nfunction ', start);
assert.ok(start >= 0 && end > start);
function node(tag, text = '') { return {tag, text, children: [], listeners: {}, value:'', append(...items){this.children.push(...items);}, setAttribute(){}, addEventListener(name,cb){this.listeners[name]=cb;}, dispatchEvent(event){this.lastEvent=event;}, focus(){this.focused=true;}}; }
function Option(text,value){return {text,value};}
class Event { constructor(type,options){this.type=type;this.bubbles=options.bubbles;} }
let notice='';
const context={element:node, document:{createElement:node}, Option, Event, notify:message=>{notice=message;}};
vm.runInNewContext(source.slice(start,end),context);
const input=node('input');
const profile={phone_national:'2025550123',phone_country_code:'+1',work_authorization:'private',instructions:'private'};
const picker=context.profileValuePicker(profile,input,{label:'Phone number',field_type:'text',sensitive:false});
assert.ok(picker);
const [select,button]=picker.children;
assert.deepEqual(Array.from(select.children, x=>x.value),['','phone_national','phone_country_code']);
button.listeners.click();assert.match(notice,/Choose/);assert.equal(input.value,'');
select.value='phone_national';button.listeners.click();assert.equal(input.value,'2025550123');assert.equal(input.lastEvent.type,'input');assert.equal(input.lastEvent.bubbles,true);assert.ok(input.focused);
select.value='work_authorization';button.listeners.click();assert.equal(input.value,'2025550123');
assert.equal(context.profileValuePicker(profile,input,{label:'Sensitive',field_type:'text',sensitive:true}),null);
assert.equal(context.profileValuePicker(profile,input,{label:'Choice',field_type:{select:{options:['a']}},sensitive:false}),null);
console.log('Profile value picker checks passed.');
