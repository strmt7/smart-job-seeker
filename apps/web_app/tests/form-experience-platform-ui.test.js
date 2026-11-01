const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const html = fs.readFileSync('apps/web_app/web/index.html', 'utf8');
const form = html.match(/<form id="experience-form"[\s\S]*?<\/form>/);
assert.ok(form, 'experience form exists');
assert.match(form[0], /<option value="workable">Workable<\/option>/);

const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf("const experiencePanel=document.querySelector('#experience-panel');");
assert.ok(start >= 0);
const requests = [];
const control = () => ({textContent:'', disabled:false, checked:false, value:'', addEventListener(type, cb){ this[type]=cb; }, replaceChildren(){}, append(){}});
const controls = new Map();
const get = key => { if(!controls.has(key)) controls.set(key, control()); return controls.get(key); };
const formControl = get('#experience-form');
formControl.elements = {confirmed:control(), use_note_in_model:control(), note:control()};
formControl.querySelector = () => control();
const data = new Map([['stage','enter_information'],['control','text'],['platform','workable'],['observation','ambiguous_label'],['response','inspect_context'],['outcome','unknown'],['confirmed','on']]);
class FormDataStub { get(key){ return data.get(key) ?? null; } has(key){ return data.has(key); } }
const context = {document:{querySelector:get}, FormData:FormDataStub, api:async(path, payload)=>{requests.push({path,payload}); return {experiences:[]};}, element:control, window:{confirm:()=>true}};
vm.runInNewContext(source.slice(start), context);
(async()=>{
 await get('#experience-related').click();
 assert.equal(requests[0].payload.platform, 'workable');
 data.delete('confirmed');
 await formControl.submit({preventDefault(){}});
 assert.ok(!requests.some(request => request.path === '/api/form-experiences/save'));
 data.set('confirmed','on');
 await formControl.submit({preventDefault(){}});
 const saved = requests.find(request => request.path === '/api/form-experiences/save');
 assert.equal(saved.payload.experience.platform, 'workable');
 assert.equal(saved.payload.confirmed, true);
 console.log('Workable lesson UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
