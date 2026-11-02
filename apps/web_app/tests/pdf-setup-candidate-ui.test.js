const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const line=source.split('\n').find(item=>item.startsWith("document.querySelector('#tool-status').addEventListener"));assert.ok(line);
let click;const button={addEventListener:(_event,fn)=>{click=fn;}},info={textContent:''},form={elements:{python:{value:''}}};
const context={document:{querySelector:id=>({'#tool-status':button,'#tool-info':info,'#pdf-setup-form':form})[id]},api:async()=>({browser:null,libreoffice:null,pdf_python:'C:/fixture/python.exe'}),notify:()=>{throw Error('unexpected error');}};
(async()=>{vm.runInNewContext(line,context);await click();assert.match(info.textContent,/PDF Python candidate/);assert.equal(form.elements.python.value,'');form.elements.python.value='C:/chosen/python.exe';await click();assert.equal(form.elements.python.value,'C:/chosen/python.exe');console.log('PDF setup diagnostics preserve automatic selection.');})().catch(error=>{console.error(error);process.exitCode=1});
