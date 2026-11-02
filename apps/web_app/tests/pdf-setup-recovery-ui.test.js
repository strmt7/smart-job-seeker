const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const a=source.indexOf('let pdfSetupFollowing=false;'),b=source.indexOf('void recoverPdfSetup()',a);assert.ok(a>=0&&b>a);
async function fixture(lostStart){
 const storage=new Map(),calls=[],info={textContent:''},button={disabled:false};let submit,active=null;
 const form={elements:{python:{value:''}},querySelector:()=>button,addEventListener:(_name,fn)=>{submit=fn;}};
 const context={document:{querySelector:s=>s==='#pdf-setup-info'?info:s==='#pdf-setup-form button'?button:form},sessionStorage:{getItem:k=>storage.get(k)||null,setItem:(k,v)=>storage.set(k,v),removeItem:k=>storage.delete(k)},setTimeout:fn=>fn(),api:async(path)=>{calls.push(path);if(path==='/api/pdf-setup/start'){active='fixture';if(lostStart)throw new Error('Lost response');return {task_id:active};}if(path==='/api/pdf-setup/pending')return {task_id:active};if(path==='/api/pdf-setup/status'){active=null;return {status:'complete',path:'C:/fictional/python.exe'};}if(path==='/api/tool-status')return {pdf_python:'C:/fictional/python.exe'};throw new Error(path);}};
 vm.runInNewContext(source.slice(a,b),context);
 await submit({preventDefault(){},currentTarget:form});
 assert.match(info.textContent,/PDF tools ready/);assert.equal(storage.size,0);assert.equal(calls.filter(p=>p==='/api/pdf-setup/start').length,1);
 if(lostStart)assert.ok(calls.includes('/api/pdf-setup/pending'));
}
(async()=>{await fixture(false);await fixture(true);console.log('PDF setup recovery UI checks passed.');})().catch(e=>{console.error(e);process.exitCode=1;});
