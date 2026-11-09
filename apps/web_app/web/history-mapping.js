/* Advisory history mapping. Only the existing reviewed-fill action touches a site. */
(() => {
 'use strict';
 const node=(tag,text,cls)=>{const el=document.createElement(tag);if(text)el.textContent=text;if(cls)el.className=cls;return el;};
 function control(result,answers,profile){
  let history;try{history=CandidateHistory.parse(profile?.career_history||'');}catch(_){return null;}
  const records=[...history.employment.map(row=>({...row,kind:'employment',label:[row.title,row.employer].filter(Boolean).join(' at ')})),...history.education.map(row=>({...row,kind:'education',label:[row.qualification,row.institution].filter(Boolean).join(' at ')}))].filter(row=>row.reviewed===true);
  const blocked=new Set(result.model_review_selectors||[]);
  const fields=(result.fields||[]).filter(field=>!field.sensitive&&!blocked.has(field.selector)&&answers.some(answer=>answer.selector===field.selector)&&(['text','text_area','month'].includes(field.field_type)||(field.field_type&&typeof field.field_type==='object'&&['number','select','radio'].some(key=>key in field.field_type))));
  if(!records.length||!fields.length)return null;
  const box=node('details',null,'history-mapping'),summary=node('summary','Suggest answers from career history'),content=node('div',null,'history-mapping-content');
  box.append(summary,content);
  content.append(node('p','Select the fields in one history section. Let Smart Jobseeker choose a checked entry, or choose it yourself. Suggestions stay in the answer editors until you review them. Dates keep their saved precision.','footnote'));
  const recordLabel=node('label','History entry'),entry=document.createElement('select');entry.setAttribute('aria-label','History entry');entry.append(new Option('Choose from saved history','auto'));entry.value='auto';
  for(const record of records)entry.append(new Option(`${record.kind==='employment'?'Employment':'Education'} · ${record.label}`,record.id));
  recordLabel.append(entry);content.append(recordLabel);
  const scope=node('fieldset'),legend=node('legend','Fields for this entry');scope.append(legend);
  const choices=[];
  const sectionPicker=document.createElement('select'),sectionLabel=node('label','Form section');
  sectionPicker.setAttribute('aria-label','Form section');sectionPicker.append(new Option('Choose fields below',''));
  const sections=new Map();
  for(const field of fields){const section=field.context?.sections?.at(-1);if(section&&!sections.has(section.id))sections.set(section.id,section.label);}
  for(const [id,label] of sections){const repeated=[...sections.values()].filter(value=>value===label).length>1;sectionPicker.append(new Option(label+(repeated?` · Section ${[...sections.keys()].indexOf(id)+1}`:''),id));}
  if(sections.size){sectionLabel.append(sectionPicker);scope.append(sectionLabel);}
  const sectionIds=[...new Set(fields.map(field=>field.context?.sections?.at(-1)?.id).filter(Boolean))];
  const duplicateLabels=new Set(fields.filter((field,index)=>fields.some((other,i)=>i!==index&&other.label===field.label)).map(field=>field.label));
  for(const field of fields){const label=node('label',null,'history-check'),check=document.createElement('input');check.type='checkbox';check.value=field.selector;label.append(check,document.createTextNode((field.label||'Unlabelled field')+(duplicateLabels.has(field.label)&&field.context?.sections?.length?` · Section ${sectionIds.indexOf(field.context.sections.at(-1).id)+1}`:'')));scope.append(label);choices.push(check);}
  sectionPicker.addEventListener('change',()=>{for(const check of choices){const field=fields.find(field=>field.selector===check.value);check.checked=!!sectionPicker.value&&field?.context?.sections?.some(section=>section.id===sectionPicker.value);check.dispatchEvent(new Event('change'));}});
  content.append(scope);
  const generate=node('button','Suggest answers'),recover=node('button','Check pending suggestions'),output=node('div',null,'history-mapping-result');generate.type=recover.type='button';content.append(generate,recover,output);
  let busy=false;
  const snapshot=()=>new Map(answers.map(answer=>[answer.selector,answer.input.value]));
  async function follow(task,baseline){
   let response;
   for(let count=0;count<180;count++){
    if(!box.isConnected)return;
    response=await api('/api/form/history-advice-status',{task_id:task});
    if(response.status!=='running')break;
    await new Promise(resolve=>setTimeout(resolve,1000));
   }
   if(!response||response.status==='running')throw Error('Suggestions are still running. Check the pending result later.');
   if(response.status!=='complete')throw Error(response.error||'History suggestions need manual review');
   const advice=response.advice;
   output.replaceChildren(node('p',`Review suggestions for ${advice.entry_label}. Check that this is the right entry and that every answer is correct.`, 'review-label'));
   if(advice.selection){const names={current:'Confirmed current entry',latest_completed:'Most recent completed entry',only_entry:'Only checked entry in this category'};output.append(node('p',`${names[advice.selection.basis]||'Suggested entry'} · ${advice.selection.evidence}`,'footnote'));}
   const proposals=[];
   for(const proposal of advice.proposals||[]){
    const target=answers.find(answer=>answer.selector===proposal.selector),value=proposal.value?.text??proposal.value?.date;
    if(!target||typeof value!=='string')continue;
    const row=node('label',null,'history-proposal'),check=document.createElement('input');check.type='checkbox';
    const changed=target.input.value!==baseline.get(proposal.selector);
    check.disabled=changed;check.checked=false;
    row.append(check,node('span',`${proposal.label}: ${value}`));
    if(changed)row.append(node('span','Your answer changed while suggestions ran; it has been kept.','review-label'));
    output.append(row);proposals.push({target,check,value,source:typeof proposal.entry_id==='string'&&typeof proposal.component==='string'&&typeof proposal.source_sha256==='string'?{entry_id:proposal.entry_id,component:proposal.component,source_sha256:proposal.source_sha256}:null,baseline:baseline.get(proposal.selector)});
   }
   if(advice.manual_review?.length)output.append(node('p',`${advice.manual_review.length} selected field(s) still need manual handling. No answer was invented for them.`,'footnote'));
   const apply=node('button','Use selected suggestions');apply.type='button';apply.disabled=!proposals.length;
   apply.addEventListener('click',async()=>{
    apply.disabled=true;
    try{
     const fresh=await api('/api/form/history-advice-status',{task_id:task});
     if(fresh.status!=='complete')throw Error(fresh.error||'The history or form review changed. Request new suggestions.');
     const selected=proposals.filter(item=>item.check.checked&&!item.check.disabled);
     if(!selected.length)throw Error('Select the suggestions you have reviewed first.');
     // Check every editor before making any changes, preserving edits atomically.
     if(selected.some(item=>item.target.input.value!==item.baseline))throw Error('An answer changed since this proposal. Your edits were kept; request new suggestions.');
     if(selected.some(item=>item.target.input.tagName==='SELECT'&&![...item.target.input.options].some(option=>option.value===item.value)))throw Error('A proposed option is no longer available. Your answers were kept; inspect again.');
     for(const item of selected){delete item.target.input._draftId;delete item.target.input._letterDraftId;item.target.input._updateLetterLink?.();item.target.input.value=item.value;item.target.input.dispatchEvent(new Event('input',{bubbles:true}));if(item.source)linkFormHistorySource(item.target.input,item.source,item.value);item.check.disabled=true;}
     notify('History answers copied for review. Editing an answer removes its saved-source link.');
    }catch(error){notify(error.message);}finally{apply.disabled=proposals.every(item=>item.check.disabled);}
   });
   output.append(apply,node('p','Suggestions are bound to the saved history and inspected session. The live page is checked again by the existing reviewed-fill action.','footnote'));
  }
  async function run(action){if(busy)return;busy=true;generate.disabled=recover.disabled=entry.disabled=sectionPicker.disabled=true;for(const check of choices)check.disabled=true;try{await action();}catch(error){output.replaceChildren(node('p',error.message,'review-label'));}finally{busy=false;generate.disabled=recover.disabled=entry.disabled=sectionPicker.disabled=false;for(const check of choices)check.disabled=false;}}
  entry.addEventListener('change',()=>output.replaceChildren());for(const check of choices)check.addEventListener('change',()=>output.replaceChildren());
  generate.addEventListener('click',()=>run(async()=>{
   const selectors=choices.filter(check=>check.checked).map(check=>check.value);
   if(!entry.value||!selectors.length||selectors.length>20)throw Error('Choose a history option and from one to twenty fields in one section.');
   const baseline=snapshot();output.replaceChildren(node('p','Mapping the selected fields locally…','footnote'));
   const started=await api('/api/form/history-advice-start',{session_id:result.session_id,entry_id:entry.value,selectors});
   await follow(started.task_id,baseline);
  }));
  recover.addEventListener('click',()=>run(async()=>{
   const pending=await api('/api/form/history-advice-pending',{});
   if(!pending.active)throw Error('No history suggestions are awaiting collection.');
   if(pending.binding?.session_id!==result.session_id)throw Error('The pending suggestions belong to another review session.');
   entry.value=pending.binding.entry_id;
   for(const check of choices)check.checked=pending.binding.selectors.includes(check.value);
   // Recovered proposals never assume an editor has stayed unchanged or empty.
   const baseline=snapshot();
   for(const [selector,value] of baseline)if(value)baseline.set(selector,Symbol('review-existing-answer'));
   await follow(pending.task_id,baseline);
  }));
  return box;
 }
 globalThis.HistoryMapping=Object.freeze({control});
})();
