const assert=require('node:assert/strict');const fs=require('node:fs');const vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const lines=source.split('\n');const use=lines.find(x=>x.startsWith('function useFormDraft(')),payload=lines.find(x=>x.startsWith('function formAnswerPayload('));assert.ok(use&&payload);
const ctx=vm.createContext({});vm.runInContext(`${lines.filter(x=>['clearFormHistorySource','linkFormHistorySource','formSuggestionUsesAnswerPayload'].some(name=>x.startsWith('function '+name+'('))).join('\n')}\n${use}\n${payload}`,ctx);
const input={value:'',focus(){this.focused=true}};ctx.useFormDraft(input,{text:'Fictional answer'},'task-1',true);assert.equal(input.value,'Fictional answer');assert.equal(input._draftId,'task-1');assert.equal(ctx.formAnswerPayload({selector:'#answer',input,kind:'text'}).draft_id,'task-1');delete input._draftId;input.value='Edited';assert.equal(ctx.formAnswerPayload({selector:'#answer',input,kind:'text'}).draft_id,undefined);input._letterDraftId='letter-1';assert.equal(ctx.formAnswerPayload({selector:'#answer',input,kind:'text'}).letter_draft_id,'letter-1');ctx.useFormDraft(input,{text:'New answer'},'task-2',false);assert.equal(input._letterDraftId,undefined);assert.equal(input._draftId,undefined);assert.ok(source.includes("input.addEventListener('input',()=>{delete input._draftId;})"));console.log('Form draft provenance UI checks passed.');

input._historySource={entry_id:'a'.repeat(32),component:'employer',source_sha256:'b'.repeat(64)};input._historySourceValue='Saved';input.value='Saved';
assert.equal(ctx.formAnswerPayload({selector:'#answer',input,kind:'text'}).history_source.component,'employer');
input.value='Edited';assert.equal(ctx.formAnswerPayload({selector:'#answer',input,kind:'text'}).history_source,undefined);
input.value='Saved';ctx.useFormDraft(input,{text:'Saved'},'task-3',true);assert.equal(input._historySource,undefined);
console.log('Saved-history payload freshness and draft replacement checks passed.');

input.value='Saved';input._historySource={component:'employer'};input._historySourceValue='Saved';
assert.equal(ctx.formSuggestionUsesAnswerPayload({input,original:'Saved'}),true,'an unchanged value still needs to send its selected source');
delete input._historySource;delete input._draftId;assert.equal(ctx.formSuggestionUsesAnswerPayload({input,original:'Saved'}),false);
