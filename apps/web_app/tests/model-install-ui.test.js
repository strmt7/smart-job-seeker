const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js','utf8');
const start = source.indexOf('let modelInstallFollowing=false;');
const end = source.indexOf('function modelCanStart(',start);
assert.ok(start >= 0 && end > start);
const store=new Map([['waypoint-model-install','fixture-task']]),calls=[],info={textContent:''};let handler;const button={disabled:false,addEventListener:(_event,fn)=>{handler=fn;}};
const context={document:{querySelector:q=>q==='#model-install'?button:info},sessionStorage:{getItem:k=>store.get(k)||null,setItem:(k,v)=>store.set(k,v),removeItem:k=>store.delete(k)},setTimeout:fn=>fn(),api:async(path,body)=>{calls.push([path,body]);return {status:'complete',result:{model:'fixture-model',already_installed:true,verified:false}};}};
(async()=>{const follow=vm.runInNewContext(`${source.slice(start,end).replace('void recoverModelInstall();','')}; followModelInstall`,context);await follow('fixture-task');assert.equal(calls[0][0],'/api/model-install/status');assert.equal(store.size,0);assert.equal(button.disabled,false);assert.match(info.textContent,/different manifest/);calls.length=0;context.document.querySelector=q=>q==='#model-install'?button:q==='#model-install-choice'?{value:'qwen3.5:9b'}:info;context.api=async(path,body)=>{calls.push([path,body]);return path.endsWith('/start')?{task_id:'new-task'}:{status:'complete',result:{model:'qwen3.5:9b',already_installed:false,verified:true}};};await handler();assert.equal(calls[0][0],'/api/model-install/start');assert.equal(calls[0][1].model,'qwen3.5:9b');assert.equal(store.size,0);assert.match(info.textContent,/installed/);console.log('Model installation UI checks passed.');})().catch(error=>{console.error(error);process.exitCode=1;});

const html=fs.readFileSync("apps/web_app/web/index.html","utf8");
assert.match(html,/value="ministral-3:14b"/);
