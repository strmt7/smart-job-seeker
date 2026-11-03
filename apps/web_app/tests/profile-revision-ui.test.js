const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js','utf8');
const line = source.split('\n').find(value=>value.startsWith("document.querySelector('#profile-form').addEventListener('submit'"));
assert.ok(line);
async function run(stale){
  let handler, sent, notice, offered=false;
  const button={disabled:false},form={querySelector:selector=>selector==='button'?button:null},editor={open:true};
  const context={profileEditRevision:'a'.repeat(64),state:{profile:{}},autoFitPaused:true,autoFitAttempted:new Set(),
    FormData:function(){return [['first_name','Fictional draft']];},
    document:{querySelector(selector){return selector==='#profile-form'?{addEventListener(_name,fn){handler=fn;}}:editor;}},
    api:async(_path,body)=>{sent=body;if(stale)throw new Error('Personal details changed in another session. Copy your edits.');return {ok:true,profile_revision:'b'.repeat(64)};},
    offerProfileRebase:async()=>{offered=true;},refresh:async()=>{},renderProfileSummary(){},scheduleAutomaticFit(){},notify(value){notice=value;}};
  vm.createContext(context);vm.runInContext(line,context);
  await handler({preventDefault(){},currentTarget:form});
  return {context,sent,notice,button,editor,offered};
}
(async()=>{
  const stale=await run(true);
  assert.equal(stale.sent._expected_profile_revision,'a'.repeat(64));
  assert.equal(stale.context.profileEditRevision,'a'.repeat(64));
  assert.equal(stale.editor.open,true);
  assert.equal(stale.offered,true);
  assert.match(stale.notice,/changed in another session/);
  assert.equal(stale.button.disabled,false);
  const saved=await run(false);
  assert.equal(saved.context.profileEditRevision,'b'.repeat(64));
  assert.equal(saved.context.state.profile.first_name,'Fictional draft');
  assert.equal(saved.editor.open,false);
  console.log('Profile revision UI checks passed.');
})().catch(error=>{console.error(error);process.exitCode=1});
