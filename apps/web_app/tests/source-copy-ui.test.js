'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
class Node {
  constructor(tag, text = '', className = '') { Object.assign(this, { tag, textContent: text, className, children: [], listeners: {}, attributes: {}, isConnected: true }); this.classList = { add: name => { this.className += ' ' + name; } }; }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = nodes; }
  setAttribute(key, value) { this.attributes[key] = value; }
  addEventListener(type, listener) { this.listeners[type] = listener; }
}
function find(root, predicate) { if (predicate(root)) return root; for (const node of root.children) { const result = find(node, predicate); if (result) return result; } }
const button = (root, text) => find(root, node => node.tag === 'button' && node.textContent === text);
const status = root => find(root, node => node.attributes.role === 'status');
const settle = () => new Promise(resolve => setImmediate(resolve));
function harness(api, onFinished) {
  const timers = [], window = {};
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../web/source-copy.js'), 'utf8'), { window, URL, document: { createElement: tag => new Node(tag) }, setTimeout: callback => { timers.push(callback); } });
  return { actions: (id, attempt, documents=[]) => window.WaypointSourceCopies.previewAction(id, attempt, {api, documents:()=>documents, element:(tag,text,style)=>new Node(tag,text,style),onFinished}), preview: (preview, options) => window.WaypointSourceCopies.previewView(preview, (tag, text, style) => new Node(tag, text, style), options), create: id => window.WaypointSourceCopies.create({ id }, { api, element: (tag, text, style) => new Node(tag, text, style), onFinished }), tick: async () => { assert(timers.length, 'Expected a scheduled status poll'); timers.shift()(); await settle(); } };
}
(async () => {
  for(const [enabled,action] of [[false,'Rehearse autofill'],[true,'Rehearse autofill'],[true,'Create recorded preview']]){
    let request;const instance=harness(async(route,body)=>{if(route.endsWith('/pending'))return {active:false};if(route.endsWith('/preview-start')){request=body;return {task_id:'history-task'};}if(route.endsWith('/status'))return {status:'complete',task_id:'history-task',job_id:'history-job'};throw Error('Unexpected history route');});
    const controls=instance.actions('history-job','source');const label=find(controls,node=>node.tag==='label'&&node.children.some(child=>child.textContent==='Use checked career history'));const checkbox=label.children[0];assert.equal(Boolean(checkbox.checked),false);checkbox.checked=enabled;await button(controls,action).listeners.click();assert.equal(request.use_saved_history===true,enabled&&action==='Rehearse autofill');assert.equal(request.career_history,undefined);assert.equal(request.use_saved_contacts,undefined);
  }
  const historyOnly=harness(async()=>{}).preview({views:[],rehearsal:{scope:'saved_candidate_materials',candidate_source:{contacts_selected:false,history_selected:true,documents:[]}}},{});
  assert(find(historyOnly,node=>node.textContent==='This saved copy uses your checked career history. Answers, later steps and server validation still need review.'));
  const staleHistory=harness(async()=>{}).preview({views:[],candidate_profile_current:false,rehearsal:{scope:'saved_candidate_materials',candidate_source:{history_selected:true,documents:[]}}},{});
  assert(find(staleHistory,node=>node.textContent==='Saved candidate information has changed since this rehearsal. Review the saved answers before using them.'));
  const pdfs=[{id:'one',name:'Candidate CV.pdf',size:1024},{id:'two',name:'Letter.pdf',size:2048},{id:'three',name:'Registration.pdf',size:512},{id:'four',name:'Template.docx',size:4096}];
  let selectedRequest;
  const selectionHarness=harness(async(route,body)=>{
    if(route.endsWith('/pending'))return {active:false};
    if(route.endsWith('/preview-start')){selectedRequest=body;return {task_id:'document-task'};}
    if(route.endsWith('/status'))return {task_id:'document-task',job_id:'document-job',status:'complete'};
    throw Error('Unexpected document route');
  });
  const picker=selectionHarness.actions('document-job','source',pdfs);
  const add=button(picker,'Add PDF');
  add.listeners.click();
  const rows=find(picker,node=>node.className==='source-document-list');
  const file=row=>find(row,node=>node.attributes['aria-label']==='Selected PDF');
  const purpose=row=>find(row,node=>node.attributes['aria-label']==='Document purpose');
  assert.equal(file(rows.children[0]).children.length,4,'Only saved PDFs are offered');
  assert.equal(file(rows.children[0]).value,'','No automatic candidate selection');
  await button(picker,'Rehearse autofill').listeners.click();
  assert.equal(selectedRequest,undefined);assert.match(status(picker).textContent,/Choose a saved PDF/);
  file(rows.children[0]).value='one';add.listeners.click();file(rows.children[1]).value='two';
  await button(picker,'Rehearse autofill').listeners.click();
  assert.equal(selectedRequest,undefined);assert.match(status(picker).textContent,/only one CV/);
  purpose(rows.children[1]).value='cover_letter';purpose(rows.children[1]).listeners.change();
  add.listeners.click();const supporting=rows.children[2];purpose(supporting).value='supporting';purpose(supporting).listeners.change();file(supporting).value='three';
  const description=find(supporting,node=>node.tag==='input');assert.equal(supporting.children[2].hidden,false);
  await button(picker,'Rehearse autofill').listeners.click();assert.equal(selectedRequest,undefined);assert.match(status(picker).textContent,/Describe/);
  description.value='Professional registration';
  const search=find(picker,node=>node.attributes['aria-label']==='Find a saved PDF');search.value='letter';search.listeners.input();
  assert.equal(file(rows.children[0]).value,'one','Filtering retains an explicitly selected PDF');
  assert(file(rows.children[0]).children.some(option=>option.value==='one'));
  await button(picker,'Rehearse autofill').listeners.click();
  assert.deepEqual(JSON.parse(JSON.stringify(selectedRequest.selected_documents)),[
    {document_id:'one',role:'cv',purpose:''},{document_id:'two',role:'cover_letter',purpose:''},{document_id:'three',role:'supporting',purpose:'Professional registration'}]);
  assert.equal(selectedRequest.use_saved_contacts,undefined,'PDF-only mode never selects contacts');
  assert.equal(Object.keys(selectedRequest).some(key=>['filename','content','path'].includes(key)),false);
  selectedRequest=undefined;file(rows.children[1]).value='one';
  await button(picker,'Rehearse autofill').listeners.click();assert.equal(selectedRequest,undefined);assert.match(status(picker).textContent,/only once/);
  await button(picker,'Create recorded preview').listeners.click();assert.equal(selectedRequest.selected_documents,undefined,'Preview ignores rehearsal selection');
  button(supporting,'Remove').listeners.click();assert.equal(rows.children.length,2);
  const materials=harness(async()=>{}).preview({views:[],rehearsal:{scope:'saved_candidate_materials',candidate_source:{contacts_selected:false,documents:[{filename:'resume.pdf',selection:{role:'cv'}}]}}},{});
  assert(find(materials,node=>node.textContent==='This saved copy uses explicitly selected PDFs. Document content, layout, later steps and server validation still need review.'));
  assert(find(materials,node=>node.textContent==='CV · resume.pdf'));

  for (const [enabled, action, expected] of [[false,'Rehearse autofill',false],[true,'Rehearse autofill',true],[true,'Create recorded preview',false]]) {
    let request;
    const instance=harness(async(route,body)=>{
      if(route.endsWith('/pending')) return {active:false};
      if(route.endsWith('/preview-start')) {request=body;return {task_id:'research-task'};}
      if(route.endsWith('/status')) return {task_id:'research-task',job_id:'research-job',status:'complete',attempt_id:'research-record'};
      throw Error('Unexpected research UI route');
    });
    const controls=instance.actions('research-job','saved-record');
    assert.equal(button(controls,action).className,'button-secondary');
    find(controls,node=>node.tag==='label'&&node.children.some(child=>child.textContent==='Research technical issues online')).children[0].checked=enabled;
    await button(controls,action).listeners.click();
    assert.equal(request.public_research===true,expected);
    assert.equal(request.rehearse===true,action==='Rehearse autofill');
  }
  for (const [enabled,action] of [[false,'Rehearse autofill'],[true,'Rehearse autofill'],[true,'Create recorded preview']]) {
    let request;
    const instance=harness(async(route,body)=>{
      if(route.endsWith('/pending'))return {active:false};
      if(route.endsWith('/preview-start')){request=body;return {task_id:'snapshot-task'};}
      if(route.endsWith('/status'))return {task_id:'snapshot-task',job_id:'snapshot-job',status:'complete'};
      throw Error('Unexpected snapshot route');
    });
    const controls=instance.actions('snapshot-job','saved-record');
    const option=find(controls,node=>node.tag==='label'&&node.children.some(child=>child.textContent==='Use saved contact details')).children[0];
    assert.equal(Boolean(option.checked),false);option.checked=enabled;
    await button(controls,action).listeners.click();
    assert.equal(request.use_saved_contacts===true,enabled&&action==='Rehearse autofill');
    assert.equal(Object.keys(request).some(key=>['profile','email','contacts'].includes(key)),false);
  }
  const degraded=harness(async()=>{}).preview({views:[],rehearsal:{scope:'fictional_saved_form',public_research:[{source_status:{degraded:true},result:{status:'needs_review'}}]}},{});
  assert(find(degraded,node=>node.textContent==='Some search services were unavailable. Results may be incomplete.'));
  const unavailable=harness(async()=>{}).preview({views:[],rehearsal:{scope:'fictional_saved_form',public_research:[{result:{status:'needs_review',reason:'no_progress'}}]}},{});
  assert(find(unavailable,node=>node.textContent==='The model could not make further progress with the available sources.'));
  const researchPanel=harness(async()=>{}).preview({views:[],rehearsal:{scope:'fictional_saved_form',public_research:[{result:{status:'advice_available',advice:'Check the documented input type.',evidence:[{url:'https://docs.example.invalid/browser'},{url:'javascript:alert(1)'}]}}]}},{});
  assert(find(researchPanel,node=>node.textContent==='Check the documented input type.'));
  assert(find(researchPanel,node=>node.textContent==='These notes do not approve or retry any application action.'));
  assert(find(researchPanel,node=>node.tag==='a'&&node.href==='https://docs.example.invalid/browser'&&node.rel==='noopener noreferrer'));
  assert.equal(find(researchPanel,node=>node.tag==='a'&&node.href?.startsWith('javascript:')),undefined);
  const digest = 'a'.repeat(64);
  const preview = { views: [
    {kind:'page',label:'Page',image:'0001.png',sha256:digest,fields:[{label:'Parent name'}]},
    {kind:'frame',label:'Qualifications',image:'0002.png',sha256:digest,fields:[{label:'Child answer',required:true}]},
  ] };
  const candidatePanel=harness(async()=>{}).preview({...preview,rehearsal:{scope:'saved_candidate_contacts',fields:[]}},{});
  assert(find(candidatePanel,node=>node.textContent==='This saved copy uses a snapshot of your contact details. Candidate documents have not been selected. Later steps and server validation still need review.'));
  assert.equal(Boolean(find(candidatePanel,node=>typeof node.textContent==='string'&&node.textContent.startsWith('Rehearsal uses fictional details'))),false);
  const noCandidateImage=harness(async()=>{}).preview({views:[],rehearsal:{scope:'saved_candidate_contacts',fields:[]}},{});
  assert(find(noCandidateImage,node=>typeof node.textContent==='string'&&node.textContent.startsWith('This saved copy uses a snapshot')));
  for (const [changes,closed] of [[{},true],[{native_closed:false},false],[{closure_reverified:false},false],[{scope:'live'},false],[{post_visual:{inspection:'uncertain',blocking_dialogs:[]}},false]]) {
    const action={view:1,status:'closed_in_saved_copy',scope:'fictional_saved_copy_only',native_closed:true,closure_reverified:true,post_visual:{inspection:'complete',blocking_dialogs:[]},followup_capture:'saved',to:2,...changes};
    const panel=harness(async()=>{}).preview({...preview,views:[preview.views[0],{...preview.views[1],after_dialog:true}],rehearsal:{scope:'fictional_saved_form',dialogs:[action],fields:[]}},{});
    assert(find(panel,node=>node.tag==='option'&&node.textContent==='Frame 1 · Qualifications · Updated'));
    assert(find(panel,node=>node.tag==='select'&&node.attributes['aria-label']==='Saved page view'));
    assert(find(panel,node=>node.textContent===(closed?'Dialog closed in saved copy.':'Dialog needs review.')));
    assert.equal(Boolean(button(panel,'View updated form')),closed);
    if(closed){button(panel,'View updated form').listeners.click();assert(find(panel,node=>node.textContent==='Child answer · Required'));}
  }
  const failedDialog=harness(async()=>{}).preview({...preview,rehearsal:{scope:'fictional_saved_form',dialogs:[{view:1,status:'needs_review',reason:'field_changed',followup_capture:'unavailable'}],fields:[]}},{});
  assert(find(failedDialog,node=>node.textContent==='Page values changed during the dialog check.'));
  assert(find(failedDialog,node=>node.textContent==='The updated form could not be saved. Further entry stopped.'));
  assert.equal(button(failedDialog,'View updated form'),undefined);
  const changedForm=harness(async()=>{}).preview({...preview,rehearsal:{scope:'fictional_saved_form',dialogs:[{view:1,status:'needs_review',reason:'form_changed',proposal:{reason:'Decline optional features'}}],fields:[]}},{});
  assert(find(changedForm,node=>node.textContent==='The form changed during dialog review.'));
  assert(find(changedForm,node=>node.textContent==='Suggested action: Decline optional features'));
  assert(!find(changedForm,node=>node.textContent==='Dialog closed in saved copy.'));
  for (const flag of [true, false, undefined, 'true']) {
    const panel = harness(async()=>{}).preview({views:[{kind:'page',label:'Page',image:'0001.png',sha256:digest,fields:[{label:'Long question',label_truncated:flag,required:true}]}]},{});
    assert(find(panel,node=>node.textContent===`Long question${flag === true ? '…' : ''} · Required`));
    assert.equal(Boolean(find(panel,node=>node.textContent==='Some field labels are shortened. Review the saved image for the full question.')),flag === true);
  }
  for (const [plan,messages] of [
    [{view:1,status:'needs_review',reason:'Local mapping unavailable'},['Contact matching needs review; no contact fields were chosen automatically.','Local mapping unavailable']],
    [{view:1,status:'proposed',proposed_fields:1,warnings:['Review this question']},['1 contact suggestion prepared. Each entry still needs verification.','Review this question']],
    [{view:1,status:'proposed',proposed_fields:2,warnings:['One','Two','Three','Four'],omitted_warnings:1},['2 contact suggestions prepared. Each entry still needs verification.','More matching notes are saved in this record.']],
  ]) {
    const panel=harness(async()=>{}).preview({...preview,rehearsal:{scope:'fictional_saved_form',fields:[],fact_plans:[plan]}},{});
    for(const message of messages)assert(find(panel,node=>node.textContent===message));
    assert.equal(find(panel,node=>node.textContent==='Four'),undefined,'Long note lists stay bounded');
    assert.equal(find(panel,node=>node.textContent==='Text and image match'),undefined,'A proposal never looks like entry verification');
  }
  for(const [status,recheck,message] of [
    ['visually_matched',{scope:'same_immutable_image',attempts:2,status:'complete',overlap_conflict:false},'A second image check was used.'],
    ['visually_matched',{scope:'same_native_target_detail',attempts:2,status:'complete',overlap_conflict:false},'A second image check was used.'],
    ['visual_unobserved',{scope:'same_immutable_image',attempts:2,status:'complete',overlap_conflict:true},'The image checks need review.'],
    ['visual_review_failed',{scope:'same_immutable_image',attempts:2,status:'failed',overlap_conflict:false},'The image checks need review.'],
    ['visually_matched',{scope:'wrong',attempts:2,status:'complete',overlap_conflict:false},null],
  ]) {
    const panel=harness(async()=>{}).preview({...preview,rehearsal:{scope:'fictional_saved_form',fields:[],visual_gate:{targets:[{view:1,label:'Field',status,transcription:{coverage_recheck:recheck}}]}}},{});
    if(message)assert(find(panel,node=>node.textContent===message));
    else assert.equal(find(panel,node=>node.textContent==='A second image check was used.'),undefined);
  }
  for(const [state,conflict,note] of [
    ['visually_matched',false,'A second image check was used.'],
    ['visual_unobserved',true,'The image checks need review.'],
  ]) {
    const calls=[];
    const panel=harness(async()=>{}).preview({...preview,rehearsal:{scope:'fictional_saved_form',fields:[{view:1,label:'Country',kind:'select',status:state,detail_image:'0084.png',detail_sha256:digest,transcription:{coverage_recheck:{scope:'same_native_field_detail',attempts:2,status:'complete',overlap_conflict:conflict}}}]}},
      {jobId:'job',attemptId:'attempt',api:async(route,body)=>{calls.push([route,body]);return {name:'0084.png',sha256:digest,image_base64:'iVBORw0KGgo='};}});
    assert(find(panel,node=>node.textContent===note));
    await button(panel,'View detail image').listeners.click();
    assert.equal(calls[0][0],'/api/attempts/screenshot');
    assert.equal(calls[0][1].name,'0084.png');
    assert(find(panel,node=>node.tag==='img'));
  }
  for (const [activity,message] of [
    [{status:'unavailable'},'Request activity was unavailable for this check.'],
    [{status:'observed',scope:'entry_and_review_interval',delta:{blocked:1}},'During this check, the saved page blocked 1 request. The website response remains unverified.'],
    [{status:'observed',scope:'entry_and_review_interval',delta:{blocked:3}},'During this check, the saved page blocked 3 requests. The website response remains unverified.'],
    [{status:'observed',scope:'entry_and_review_interval',delta:{blocked:0}},null],
    [{status:'observed',scope:'wrong',delta:{blocked:1}},null],
    [{status:'observed',scope:'entry_and_review_interval',delta:{blocked:'secret-url'}},null],
  ]) {
    const panel=harness(async()=>{}).preview({...preview,rehearsal:{scope:'fictional_saved_form',fields:[{view:1,label:'Upload',kind:'file',status:'visual_unobserved',replay_activity:activity}]}},{});
    assert(find(panel,node=>node.textContent==='Upload · Selected file needs visual review'));
    if(message)assert(find(panel,node=>node.textContent===message));
    else assert.equal(find(panel,node=>node.textContent.includes('During this check')),undefined);
    assert.equal(find(panel,node=>node.textContent==='Attachment and image match'),undefined);
  }
  for (const [native, state, visible] of [[true,'visually_matched',true],[false,'visually_matched',false],[true,'entry_unverified',false]]) {
    const formatted = harness(async () => {}).preview({...preview, rehearsal:{scope:'fictional_saved_form',fields:[{
      view:1,label:'International contact',kind:'text',native_verified:native,status:state,value_comparison:'international_telephone_presentation',
    }]}},{});
    assert.equal(Boolean(find(formatted,node=>node.textContent==='The form formats this number differently; the digits match.')),visible);
  }
  let release;
  const panel = harness(async () => {}).preview(preview,{jobId:'job',attemptId:'attempt',api:async (_route,body) => {
    if(body.name==='0001.png') return new Promise(resolve => { release=resolve; });
    return {name:body.name,sha256:'b'.repeat(64),image_base64:'fixture'};
  }});
  const select = find(panel,node => node.tag==='select');
  const show = button(panel,'View image');
  const first = show.listeners.click();
  select.value='1'; select.listeners.change();
  release({name:'0001.png',sha256:digest,image_base64:'fixture'});
  await first;
  assert.equal(find(panel,node => node.tag==='img'),undefined,'A previous selection cannot replace the current view');
  assert(find(panel,node => node.textContent==='Child answer · Required'));
  await show.listeners.click();
  assert(find(panel,node => node.textContent==='The saved image differs from its record.'));
  assert.equal(find(panel,node => node.tag==='img'),undefined);

  const gated = harness(async () => {}).preview({...preview, rehearsal:{scope:'fictional_saved_form',halted:true,fields:[],visual_gate:{required:true,available:false,targets:[{view:1,phase:'before_entry',label:'Parent name',status:'visual_unobserved',image:'0020.png',sha256:digest}]}}},{});
  assert(find(gated,node=>node.textContent==='A loaded local vision model is required before autofill. Check the model settings.'));
  assert(find(gated,node=>node.textContent==='Before entry \u00b7 Parent name \u00b7 Visual inspection needs review'));
  assert(button(gated,'View target image'));
  assert.equal(find(gated,node=>node.textContent==='Before entry \u00b7 Parent name \u00b7 Target and image match'),undefined);

  const blocked = harness(async () => {}).preview({views:[{
    ...preview.views[0], walls:['Custom country needs review'],wall_count:10,omitted_walls:2,
    independent_entry:{blocked:true,barriers:['captcha present']},
  }],rehearsal:{scope:'fictional_saved_form',fields:[
    {view:1,label:'Parent name',status:'before_entry_review_required',review_reason:'Image label could not be matched'},
    {view:1,label:'Other name',status:'entry_unverified'},
  ],sections:[{view:1,status:'stopped',reason:'page_needs_review'}]}},{});
  for (const text of ['2 additional review notes are recorded for this view.', 'Entry stopped before filling fields in this view.',
    'captcha present','Parent name · Visual review stopped entry before any value was entered',
    'Other name · Entry could not be verified','Image label could not be matched','This section needs review before continuing.']) {
    assert(find(blocked,node=>node.textContent===text),text);
  }
  assert.equal(find(blocked,node=>node.textContent.includes('Text verified in form')),undefined);
  const notes = find(blocked,node=>node.tag==='details' && node.children.some(child=>child.tag==='summary' && child.textContent==='Section review notes'));
  assert(notes, 'Secondary review notes use the shared disclosure pattern');
  assert(find(notes,node=>node.tag==='li' && node.textContent==='Custom country needs review'));
  assert.equal(find(notes,node=>node.textContent==='captcha present'),undefined,'An entry barrier remains visible outside the collapsed review notes');
  const independent = harness(async () => {}).preview({views:[{
    ...preview.views[0],walls:['Custom country needs review'],wall_count:1,omitted_walls:1,
    independent_entry:{blocked:false,barriers:[]},
  }]},{});
  assert(find(independent,node=>node.textContent==='Supported fields can be tested independently. Review notes still prevent completing this section.'));
  assert(find(independent,node=>node.textContent==='1 additional review note is recorded for this view.'));
  assert.equal(find(independent,node=>node.textContent==='Entry stopped before filling fields in this view.'),undefined);

  const rehearsal = {...preview, rehearsal: {scope:'fictional_text_only', fields:[
    {view:2,label:'Child answer',status:'visual_unavailable',image:'0006.png',sha256:digest},
    {view:2,label:'Unclear field',status:'visual_review_failed'},
    {view:2,label:'Clear field',status:'visually_matched'},
  ]}};
  const tested = harness(async () => {}).preview(rehearsal,{jobId:'job',attemptId:'test',api:async (_route,body) => {
    assert.equal(body.name,'0006.png');
    return {name:body.name,sha256:digest,image_base64:'fixture'};
  }});
  assert.equal(find(tested,node => node.textContent==='View field image'),undefined);
  const testedSelect = find(tested,node => node.tag==='select');
  testedSelect.value='1'; testedSelect.listeners.change();
  assert(find(tested,node => node.textContent==='Child answer · Text verified in form · visual review unavailable'));
  assert(find(tested,node => node.textContent==='Unclear field · Text verified in form · image review failed'));
  assert(find(tested,node => node.textContent==='Clear field · Text and image match'));
  for (const [plan, message] of [
    [{view:1,status:'needs_review'}, 'Document matching needs review; no attachment was chosen automatically.'],
    [{view:1,status:'proposed',decisions:[{document_id:null}]}, '1 upload control needs a document choice.'],
    [{view:1,status:'proposed',decisions:[{document_id:null},{document_id:null}]}, '2 upload controls need a document choice.'],
  ]) {
    const unchosen = harness(async () => {}).preview({...rehearsal,rehearsal:{...rehearsal.rehearsal,attachment_plans:[plan]}},{});
    assert(find(unchosen,node=>node.textContent===message));
    assert.equal(find(unchosen,node=>node.textContent==='Attachment and image match'),undefined);
  }
  await button(tested,'View field image').listeners.click();
  assert(find(tested,node => node.tag==='img'));
  const choice = harness(async () => {}).preview({
    ...preview, rehearsal: {scope:'fictional_saved_form', fields:[
      {view:2,kind:'select',label:'Country',status:'visually_matched'},
      {view:2,kind:'radio',label:'City',status:'visual_unavailable'},
      {view:2,kind:'file',label:'Resume',status:'visual_unavailable',attachment:{name:'sample-resume.pdf'}},
      {view:2,kind:'file',label:'Cover letter',status:'visual_unobserved'},
      {view:2,kind:'file',label:'Certificate',status:'visual_difference'},
    ]},
  }, {jobId:'job',attemptId:'test',api:async () => {}});
  const choiceSelect = find(choice,node => node.tag==='select');
  choiceSelect.value='1'; choiceSelect.listeners.change();
  assert(find(choice,node => node.textContent==='Country · Selection and image match'));
  assert(find(choice,node => node.textContent==='City · Selection verified in form · visual review unavailable'));
  assert(find(choice,node => node.textContent==='Resume · Attachment verified in form · visual review unavailable'));
  assert(find(choice,node => node.textContent==='Cover letter · Selected file needs visual review'));
  assert(find(choice,node => node.textContent==='Certificate · Selected file needs visual review'));
  assert(find(choice,node => node.textContent==='sample-resume.pdf'));
  const stages = harness(async () => {}).preview({
    views: [...preview.views, {kind:'frame', label:'Qualifications', scope_view:2, section:2,
      image:'0003.png', sha256:digest, fields:[{label:'Later answer'}]}],
    rehearsal: {scope:'fictional_saved_form', fields:[{view:3,label:'Later answer',status:'visual_unavailable'}],
      sections:[{view:2,status:'advanced',to:3,control:'Next step'},
        {view:3,status:'stopped',reason:'required_fields_unverified'}]},
  }, {jobId:'job',attemptId:'stages',api:async () => {}});
  const stageSelect = find(stages,node => node.tag==='select');
  stageSelect.value='1'; stageSelect.listeners.change();
  assert(find(stages,node => node.textContent==='Next step opened section 2 in the saved copy.'));
  stageSelect.value='2'; stageSelect.listeners.change();
  assert(find(stages,node => node.textContent==='Frame 1 · Qualifications · Section 2'));
  assert(find(stages,node => node.textContent==='Required fields need review before continuing.'));
  assert(find(stages,node => node.textContent==='Later answer · Text verified in form · visual review unavailable'));
})().catch(error=>{console.error(error);process.exitCode=1;});

(async () => {
  let finish, starts = 0, pending = 0;
  const h = harness(async route => {
    if (route.endsWith('/pending')) { pending++; return { active: false }; }
    if (route.endsWith('/start')) { starts++; return { task_id: 'copy-a' }; }
    if (route.endsWith('/status')) return { task_id: 'copy-a', job_id: 'a', status: 'complete', attempt_id: 'record-a', result: {} };
    throw Error('Unexpected route');
  }, () => new Promise(resolve => { finish = resolve; }));
  const first = h.create('a'), second = h.create('b');
  await settle(); assert.equal(pending, 1, 'Concurrent mounts share recovery');
  const click = button(first, 'Save page copy').listeners.click();
  await settle();
  assert.equal(starts, 1); assert(finish);
  assert.equal(button(first, 'Save another copy').disabled, true, 'Completion refresh keeps start disabled');
  assert.equal(button(second, 'Save page copy').disabled, true);
  await button(second, 'Save page copy').listeners.click(); assert.equal(starts, 1);
  finish(); await click;
  assert.equal(button(first, 'Save another copy').disabled, false);
  assert.match(status(first).textContent, /Page copy incomplete/);
  assert.match(status(first).textContent, /Check the saved record/);
  assert.doesNotMatch(status(first).textContent, /0 resources/);

  for (const pending of [true, false, undefined, 'true']) {
    const inspection = harness(async route => {
      if (route.endsWith('/pending')) return {active:false};
      if (route.endsWith('/start')) return {task_id:'style-copy'};
      if (route.endsWith('/status')) return {task_id:'style-copy',job_id:'style-job',status:'complete',attempt_id:'style-record',result:{needs_review:true,report:{recorded_resources:2,records:[],discovery:{rounds:[{stylesheet_inspection_pending:pending}]}}}};
      throw Error('Unexpected route');
    });
    const panel=inspection.create('style-job');await settle();
    await button(panel,'Save page copy').listeners.click();
    const warning=find(panel,node=>node.textContent==='Some styling could not be inspected. Review this copy.');
    assert.equal(Boolean(warning),pending===true);
    if (warning) assert.equal(warning.className,'review-label');
  }

  let cancelled = false;
  const active = harness(async route => {
    if (route.endsWith('/pending')) return { active: true, task_id: 'copy-b', job_id: 'b' };
    if (route.endsWith('/cancel')) throw Error('Stop request could not reach the server.');
    if (route.endsWith('/status')) return cancelled ? { status: 'cancelled', task_id: 'copy-b', job_id: 'b' } : { status: 'running', task_id: 'copy-b', job_id: 'b', phase: 'resources' };
    throw Error('Unexpected route');
  });
  const own = active.create('b'), other = active.create('c'); await settle();
  assert.equal(find(own, node => node.tag === 'progress').hidden, false);
  assert.equal(find(other, node => node.tag === 'progress').hidden, true);
  await button(other, 'Stop current copy').listeners.click();
  assert.match(status(other).textContent, /Stop request could not reach the server/);
  assert.equal(button(other, 'Stop current copy').disabled, false);
  cancelled = true; await active.tick();
  assert.match(status(own).textContent, /^Stopped\./);
  assert.equal(button(own, 'Save another copy').disabled, false);

  const mismatch = harness(async route => route.endsWith('/pending') ? { active: true, task_id: 'expected', job_id: 'a' } : { task_id: 'wrong', job_id: 'a', status: 'complete' });
  const refused = mismatch.create('a'); await settle();
  assert.match(status(refused).textContent, /does not match this job/);
  assert.doesNotMatch(status(refused).textContent, /Page copy saved/);
  const html = fs.readFileSync(path.join(__dirname, '../web/index.html'), 'utf8');
  assert(html.indexOf('/source-copy.js') < html.indexOf('/app.js') && html.includes('/source-copy.js'));
  console.log('Source-copy shared recovery, refresh race, unknown accounting, cancellation errors and result binding passed.');
})().catch(error => { console.error(error); process.exitCode = 1; });
