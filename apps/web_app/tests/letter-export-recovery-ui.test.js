const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js','utf8');
const start = source.indexOf('let letterExportRecovering=false;');
const end = source.indexOf('\nvoid recoverPendingLetterExport(',start);
assert.ok(start >= 0 && end > start);
async function fixture(active, saved){
 const storage=new Map(saved ? [['waypoint-letter-export',JSON.stringify(saved)]] : []), calls=[], shown=[];
 const panel={replaceChildren(){},append(){}};
 const context={sessionStorage:{getItem:key=>storage.get(key)||null,setItem:(key,value)=>storage.set(key,value),removeItem:key=>storage.delete(key)},element:()=>({}),
  api:async(path)=>{calls.push(path);return active;},followLetterExport:async(task)=>{calls.push(task);storage.delete('waypoint-letter-export');return {id:'fictional',name:'fictional.pdf'};},
  showExportedPdf:(_,result)=>shown.push(result),refresh:async()=>{},notify:()=>{}};
 const recover=vm.runInNewContext(`${source.slice(start,end)}; recoverPendingLetterExport`,context);
 return {recovered:await recover(panel),calls,shown,storage};
}
(async()=>{
 const running=await fixture({task_id:'new-task',job_id:'fictional-job'}, {task:'old-task',job:'old-job'});
 assert.equal(running.recovered,true);assert.deepEqual(running.calls,['/api/letter/export-pending','new-task']);assert.equal(running.shown.length,1);assert.equal(running.storage.size,0);
 const replay=await fixture({task_id:null},{task:'saved-task',job:'fictional-job'});
 assert.equal(replay.recovered,true);assert.equal(replay.calls[1],'saved-task');
 const empty=await fixture({task_id:null},null);assert.equal(empty.recovered,false);assert.equal(empty.shown.length,0);
 console.log('PDF export recovery UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1;});
