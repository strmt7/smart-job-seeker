const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const line = source.split('\n').find(item => item.startsWith("document.querySelector('#office-path-form').addEventListener"));
assert.ok(line);
const button = {disabled:false}, info = {textContent:''};
const form = {elements:{path:{value:' C:\\Apps\\LibreOffice\\program\\soffice.com '}},querySelector(){return button;},addEventListener(_event, handler){this.submit=handler;}};
let request;
const context = {document:{querySelector(selector){return selector==='#office-path-form'?form:info;}},api:async(path,body)=>{request={path,body};return {active:body.path};}};
vm.runInNewContext(line,context);
(async()=>{
 await form.submit({preventDefault(){},currentTarget:form});
 assert.equal(request.path,'/api/office-path');
 assert.equal(request.body.path,'C:\\Apps\\LibreOffice\\program\\soffice.com');
 assert.match(info.textContent,/Run a conversion to verify/);
 assert.equal(button.disabled,false);
 console.log('Office path UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
