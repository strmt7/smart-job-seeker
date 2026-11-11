/* Shared page-copy state survives job-list rerenders without duplicate polling. */
(() => {
  const views = new Map();
  const state = { task: null, job: null, preview: false, active: false, phase: 'starting', cancelling: false, result: null, error: null };
  let client, monitor = null, recovering = null, finished;
  const phases = { starting: 'Starting page copy…', tools: 'Setting up background tools…', reading: 'Reading the original page…', resources: 'Collecting page resources…', preview: 'Rendering the saved page…', rehearsal: 'Checking autofill in the saved copy…', saved: 'Finishing saved records…' };
  function publish() {
    for (const [node, render] of views) {
      if (node.isConnected) render(); else views.delete(node);
    }
  }
  function count(value) { return Number.isSafeInteger(value) && value >= 0 ? value : null; }
  function questionPreparation(control, el, options) {
    const root=el('div','','source-question-preparation'), open=el('button','Prepare answer','secondary');
    open.type='button';root.append(open);
    const identity={id:options.jobId,attempt_id:options.attemptId,number:control.number};
    open.addEventListener('click',async()=>{
      if(open.disabled)return;open.disabled=true;
      const panel=el('section','','source-question-review'), status=el('p','Loading saved question…','footnote');
      status.setAttribute('role','status');root.replaceChildren(open,panel);panel.append(status);
      try {
        const context=await options.api('/api/job-source/question',identity);
        if(typeof context.question!=='string'||typeof context.digest!=='string')throw Error('The saved question could not be verified.');
        panel.replaceChildren(el('h4','Answer preparation'),el('p',context.question,'source-question-text'),el('p',context.notice,'footnote'));
        function showDraft(answer,caption,container=panel){
          const value=el('textarea');value.value=typeof answer?.text==='string'?answer.text:'';
          value.readOnly=true;value.rows=5;value.setAttribute('aria-label','Prepared answer');
          container.append(el('p',caption,'footnote'),value);
          if(typeof options.renderEvidence==='function')options.renderEvidence(container,answer);
          else container.append(el('p','Review every claim against the supporting qualifications.','review-label'));
          const sources=Array.isArray(answer?.sources)?answer.sources.filter(source=>typeof source==='string').slice(0,10):[];
          if(sources.length){const facts=el('details'),summary=el('summary',`Cited qualifications (${sources.length})`);facts.append(summary);for(const source of sources)facts.append(el('p',source,'footnote'));container.append(facts);}
          container.append(el('p','Evidence checks are advisory. Confirm the facts and wording before using this answer.','footnote'));
        }
        for(const draft of Array.isArray(context.drafts)?context.drafts.slice(0,8):[])showDraft(draft.answer,draft.current?'Saved draft · supporting information unchanged':'Historical draft · supporting information changed');
        if(context.drafts_partial)panel.append(el('p','More saved files may be present in this application record.','footnote'));
        const start=el('button','Draft answer','secondary'), latest=el('div');start.type='button';panel.append(start,status,latest);
        status.textContent='Review the question above. The draft uses your reviewed qualifications and writing preferences.';
        start.addEventListener('click',async()=>{
          if(start.disabled)return;start.disabled=true;status.textContent='Preparing answer with your local model…';
          try {
            const started=await options.api('/api/job-source/answer-start',{...identity,digest:context.digest});
            if(typeof started.task_id!=='string')throw Error('The answer task could not be verified.');
            // Keep exactly this finite task observed even if its view is
            // rerendered. Completion archives the draft; no global timer/cache.
            const deadline=Date.now()+600000;
            while(Date.now()<deadline){
              const result=await options.api('/api/job-source/answer-status',{...identity,task_id:started.task_id});
              if(result.status==='complete'){
                latest.replaceChildren();showDraft(result.answer,`Saved in this application record: ${result.saved_file}`,latest);
                status.textContent='Draft saved. Review its accuracy and the current question before use.';return;
              }
              if(result.status!=='running')throw Error(result.error||'The answer could not be prepared.');
              await new Promise(resolve=>setTimeout(resolve,1000));
            }
            throw Error('Answer preparation timed out. Check the application record before trying again.');
          } catch(error){status.textContent=error.message;}
          finally {start.disabled=false;}
        });
      } catch(error){status.textContent=error.message;open.disabled=false;}
    });
    return root;
  }
  function outlineView(outline, el, options={}) {
    if (!outline) return el('p', 'Controls were not assessed for this copy.', 'footnote');
    if (!['observed','partial'].includes(outline.status) || !Array.isArray(outline.controls) || outline.controls.length>256 || outline.controls.some(c=>!c||typeof c!=='object'))
      return el('p', 'Page controls could not be inspected. Review the saved copy manually.', 'footnote');
    const root=el('details','','source-form-outline'), summary=el('summary',`Page controls (${outline.controls.length} observed)`), list=el('ol','','source-form-controls');
    root.append(summary,el('p','Controls exposed by the saved page. Later steps, embedded forms and unsupported controls may be missing.','footnote'));
    if (!outline.controls.length) root.append(el('p','No supported controls were observed. Check the original page for an application link.','footnote'));
    const roles={textbox:'Text field',searchbox:'Search field',combobox:'Choice',listbox:'Choice list',checkbox:'Checkbox',radio:'Single choice',switch:'Toggle',spinbutton:'Number',slider:'Range',button:'Button',link:'Link'};
    let expanded=false;
    function render() {
      list.replaceChildren();
      for (const control of outline.controls.slice(0,expanded?256:12)) {
        const row=el('li');
        if(Array.isArray(control.context)&&control.context.length)row.append(el('p',control.context.join(' · '),'footnote'));
        row.append(el('strong',typeof control.label==='string'&&control.label?control.label:'Unlabelled control'));
        const requirement=control.required===true?'Required':control.required===false?'Optional':'Requirement not exposed';
        const state=control.disabled===true?'Disabled':control.disabled===false?'Enabled':'State not exposed';
        const metadata=[typeof roles[control.role]==='string'?roles[control.role]:'Control'];
        const action=control.role==='link'||control.role==='button';
        if(!action)metadata.push(requirement);
        if(!action||typeof control.disabled==='boolean')metadata.push(state);
        row.append(el('p',metadata.join(' · '),'footnote'));
        if(typeof control.description==='string'&&control.description)row.append(el('p',control.description,'footnote'));
        if(control.context_incomplete)row.append(el('p','The surrounding question context is incomplete.','footnote'));
        if(options.api&&options.jobId&&options.attemptId&&control.role==='textbox'&&control.label&&control.disabled!==true&&!control.context_incomplete)
          row.append(questionPreparation(control,el,options));
        list.append(row);
      }
    }
    render();root.append(list);
    if(outline.controls.length>12){const more=el('button',`Show all ${outline.controls.length} controls`,'secondary');more.type='button';more.setAttribute('aria-expanded','false');more.addEventListener('click',()=>{expanded=!expanded;render();more.textContent=expanded?'Show fewer controls':`Show all ${outline.controls.length} controls`;more.setAttribute('aria-expanded',String(expanded));});root.append(more);}
    if(outline.status==='partial')root.append(el('p','Some controls, labels or question context could not be retained. Review the original page.','footnote'));
    return root;
  }
  function previewView(preview, el, options) {
    const section = el(options.inline ? 'div' : 'details', '', 'source-copy-result');
    const rehearsal = ['fictional_text_only', 'fictional_saved_form'].includes(preview?.rehearsal?.scope) ? preview.rehearsal : null;
    if (!options.inline) section.append(el('summary', rehearsal ? 'Offline autofill rehearsal' : 'Recorded page preview'));
    const captures = Array.isArray(preview?.views) ? preview.views.filter(view => view && ['page','frame'].includes(view.kind)).slice(0, 7) : [];
    if (!captures.length) {
      section.append(el('p', 'No view could be saved. Check this record’s screenshots and status.', 'review-label'));
      return section;
    }
    const label = el('label', 'View', 'source-copy-view'), select = el('select');
    const frames = new Map();
    for (const [index, view] of captures.entries()) {
      const scope = count(view.scope_view) || index + 1;
      if (view.kind === 'frame' && !frames.has(scope)) frames.set(scope, frames.size + 1);
      const heading = view.kind === 'page' ? 'Page' : `Frame ${frames.get(scope)} · ${view.label || view.origin || 'Embedded page'}`;
      const stage = count(view.section);
      const option = el('option', stage > 1 ? `${heading} · Section ${stage}` : heading);
      option.value = String(index);
      select.append(option);
    }
    label.append(select);
    const details = el('div'), imageArea = el('div');
    const show = el('button', 'View image', 'secondary');
    show.type = 'button';
    const unsupported = preview.replay?.unsupported_targets;
    const hasUnsupported = unsupported && typeof unsupported === 'object' && Object.values(unsupported).some(value => count(value) > 0);
    if (preview.truncated || preview.uninspected_frames || preview.replay?.blocked || hasUnsupported) {
      section.append(el('p', 'Some frames or resources were unavailable in this copy.', 'review-label'));
    }
    for (const notice of (Array.isArray(preview.notices) ? preview.notices.slice(0, 5) : [])) {
      if (typeof notice === 'string') section.append(el('p', notice, 'footnote'));
    }
    if (rehearsal) {
      section.append(el('p', 'Rehearsal uses fictional details in the saved copy. Other fields, later steps and server validation still need review.', 'footnote'));
      if (rehearsal.halted) section.append(el('p', 'The rehearsal stopped for review.', 'review-label'));
      if (rehearsal.visual_gate?.required && !rehearsal.visual_gate.available) {
        section.append(el('p', 'A loaded local vision model is required before autofill. Check the model settings.', 'review-label'));
      }
    }
    section.append(label, details, show, imageArea);
    let revision = 0;
    function describeSelection() {
      ++revision;
      const view = captures[Number(select.value)];
      imageArea.replaceChildren();
      show.disabled = false;
      details.replaceChildren();
      if (!view) return;
      if (view.origin) details.append(el('p', view.origin, 'footnote'));
      const fields = Array.isArray(view.fields) ? view.fields.filter(field => field && typeof field.label === 'string').slice(0, 40) : [];
      const total = count(view.field_count);
      if (total > fields.length) details.append(el('p', `Showing ${fields.length} of ${total} fields.`, 'footnote'));
      else if (!fields.length) details.append(el('p', 'No form fields were detected in this view.', 'footnote'));
      if (fields.length) {
        const list = el('ul');
        for (const field of fields) list.append(el('li', `${field.label || 'Unlabelled field'}${field.required ? ' · Required' : ''}`));
        details.append(list);
      }
      for (const wall of (Array.isArray(view.walls) ? view.walls.slice(0, 8) : [])) {
        if (typeof wall === 'string') details.append(el('p', wall, 'review-label'));
      }
      if (rehearsal) {
        const viewNumber = Number(select.value) + 1;
        const checks = Array.isArray(rehearsal.fields) ? rehearsal.fields.slice(0, 8).filter(field => field?.view === viewNumber) : [];
        if (!checks.length) details.append(el('p', 'No fields were tested in this view.', 'footnote'));
        for (const check of checks) {
          const stateLabel = check.kind === 'file' ? 'Attachment' : ['select', 'radio'].includes(check.kind) ? 'Selection' : 'Text';
          const states = {
            visually_matched: `${stateLabel} and image match`,
            visual_difference: 'Image transcription differs',
            visual_unobserved: 'Model could not read the image',
            visual_review_failed: `${stateLabel} verified in form · image review failed`,
            visual_unavailable: `${stateLabel} verified in form · visual review unavailable`,
            changed_after_entry: 'Field could not be reverified after entry',
            readback_unavailable: 'Field needs manual review',
          };
          const row = el('div', '', 'source-copy-actions');
          row.append(el('span', `${check.label || 'Field'} · ${states[check.status] || 'Needs review'}`, 'footnote'));
          if (check.kind === 'file' && /^[a-zA-Z0-9][a-zA-Z0-9_.-]{0,95}\.pdf$/.test(check.attachment?.name || '')) {
            row.append(el('span', check.attachment.name, 'footnote'));
          }
          if (check.image) {
            const button = el('button', 'View field image', 'secondary');
            button.type = 'button';
            button.addEventListener('click', () => showImage(check, button));
            row.append(button);
          }
          details.append(row);
        }
        const targets = Array.isArray(rehearsal.visual_gate?.targets) ? rehearsal.visual_gate.targets.slice(0, 10).filter(target => target?.view === viewNumber) : [];
        for (const target of targets) {
          const phase = target.phase === 'before_section' ? 'Before section change' : 'Before entry';
          const state = target.status === 'visually_matched' ? 'Target and image match' : 'Visual inspection needs review';
          const row = el('div', '', 'source-copy-actions');
          row.append(el('span', `${phase} \u00b7 ${target.label || 'Control'} \u00b7 ${state}`, 'footnote'));
          if (target.image) {
            const button = el('button', 'View target image', 'secondary');
            button.type = 'button';
            button.addEventListener('click', () => showImage(target, button));
            row.append(button);
          }
          details.append(row);
        }
        const transition = Array.isArray(rehearsal.sections) ? rehearsal.sections.slice(0, 7).find(item => item?.view === viewNumber) : null;
        if (transition?.status === 'advanced' && typeof transition.control === 'string') {
          const target = captures[(count(transition.to) || 0) - 1];
          if (target?.section) details.append(el('p', `${transition.control} opened section ${target.section} in the saved copy.`, 'footnote'));
        } else if (transition?.status === 'stopped') {
          const reasons = {
            no_forward_control: 'No further section control was found.',
            incomplete_controls: 'The section controls could not be fully inspected.',
            ambiguous_control: 'More than one possible forward control needs review.',
            unsupported_control: 'The forward control could not be used safely in rehearsal.',
            required_fields_unverified: 'Required fields need review before continuing.',
            limit_reached: 'The rehearsal reached its section or field limit.',
            control_changed: 'The forward control changed during inspection.',
            document_changed: 'The saved document changed during navigation.',
            page_needs_review: 'The next view has an inspection warning.',
            field_changed: 'A field could not be reverified before continuing.',
            replay_gap: 'The next action needed a request the rehearsal could not replay safely.',
            no_new_section: 'The forward action exposed no new form fields.',
            section_capture_failed: 'The next section image could not be saved.',
          };
          details.append(el('p', reasons[transition.reason] || 'The next section needs review.', transition.reason === 'no_forward_control' ? 'footnote' : 'review-label'));
        }
      }
    }
    select.addEventListener('change', describeSelection);
    show.addEventListener('click', () => showImage(captures[Number(select.value)], show));
    async function showImage(view, button) {
      const requested = ++revision;
      button.disabled = true;
      imageArea.replaceChildren(el('p', 'Loading saved image…', 'footnote'));
      try {
        if (!view || !/^\d{4}\.png$/.test(view.image) || !/^[0-9a-f]{64}$/.test(view.sha256)) throw new Error('The saved image reference is invalid.');
        const image = await options.api('/api/attempts/screenshot', { id: options.jobId, attempt_id: options.attemptId, name: view.image });
        if (requested !== revision) return;
        if (image.name !== view.image || image.sha256 !== view.sha256 || typeof image.image_base64 !== 'string' || image.image_base64.length > 8388612) throw new Error('The saved image differs from its record.');
        const img = el('img', '', 'attempt-screenshot inspectable-image');
        img.alt = `Saved view of ${view.label || 'page'}`;
        img.tabIndex = 0;
        img.addEventListener('error', () => { if (requested === revision) imageArea.replaceChildren(el('p', 'The image could not be displayed.', 'review-label')); });
        img.src = `data:image/png;base64,${image.image_base64}`;
        imageArea.replaceChildren(img);
      } catch (error) {
        if (requested === revision) imageArea.replaceChildren(el('p', error.message, 'review-label'));
      } finally {
        button.disabled = false;
      }
    }
    select.value = '0';
    describeSelection();
    return section;
  }
  function previewAction(jobId, attemptId, dependencies) {
    const root = dependencies.element('div', '', 'source-copy-actions');
    const status = dependencies.element('span', '', 'footnote');
    status.setAttribute('role', 'status');
    for (const [label, rehearse] of [['Create recorded preview', false], ['Rehearse autofill', true]]) {
      const button = dependencies.element('button', label, 'secondary');
      button.type = 'button';
      if (rehearse) button.title = 'Use fictional details to check supported fields in the saved copy';
      root.append(button);
      button.addEventListener('click', async () => {
        if (state.active) {
          status.textContent = 'Wait for the current page task to finish.';
          return;
        }
        client = dependencies.api;
        finished = dependencies.onFinished;
        button.disabled = true;
        status.textContent = '';
        try {
          await start(jobId, attemptId, rehearse);
          if (state.job === jobId && state.error) status.textContent = state.error;
        } finally {
          button.disabled = false;
        }
      });
    }
    root.append(status);
    return root;
  }
  async function start(job, attempt = null, rehearse = false) {
    if (state.active) return;
    Object.assign(state, { task: null, job, preview: Boolean(attempt), active: true, cancelling: false, phase: 'starting', result: null, error: null });
    publish();
    try {
      const pending = await client('/api/job-source/pending', {});
      if (pending.active) { await follow(pending.task_id, pending.job_id, pending.preview === true); return; }
      const route = attempt ? '/api/job-source/preview-start' : '/api/job-source/start';
      const value = await client(route, attempt ? { id: job, attempt_id: attempt, ...(rehearse ? { rehearse } : {}) } : { id: job });
      await follow(value.task_id, job, Boolean(attempt));
    } catch (error) {
      state.active = false;
      state.error = error.message;
      publish();
      recover();
    }
  }
  function description(value) {
    if (value.status === 'cancelled') return 'Stopped. Available files remain in application records.';
    if (value.status !== 'complete') return value.error || 'The page copy could not finish. Available files remain in application records.';
    if (value.kind === 'recorded_preview') {
      if (!value.record_saved) return 'Check the application record folder for available images.';
      const name = value.result?.preview?.rehearsal ? 'Offline autofill rehearsal' : 'Recorded preview';
      return value.current_source === false ? `${name} saved for the earlier job link.` : `${name} saved in application records.`;
    }
    const report = value.result?.report, resources = count(report?.recorded_resources), requests = count(report?.requests);
    const counts = resources === null || requests === null ? 'Check the saved record for capture details.' : `${resources} resource${resources === 1 ? '' : 's'} saved from ${requests} request${requests === 1 ? '' : 's'}.`;
    const review = value.result?.needs_review !== false || value.result?.archive_written !== true;
    const heading=value.result?.archive_written!==true?'Page copy incomplete. ':review?'Page copy saved with gaps. ':'Page copy saved. ';
    return `${heading}${counts} ${value.current_source === false ? 'The job URL changed; this copy belongs to the earlier source. ' : value.current_source!==true?'The current job URL could not be verified. ':''}${value.record_saved === false ? 'The record could not be finalized; check its folder. ' : ''}${value.review_flag_saved===false?'The job’s review flag could not be saved. ':''}Unopened sections and server checks are not verified.`;
  }
  async function follow(task, job, preview = false) {
    if (monitor) return monitor;
    Object.assign(state, { task, job, preview, active: true, result: null, error: null }); publish();
    monitor = (async () => {
      try {
        for (;;) {
          const result = await client('/api/job-source/status', { task_id: task });
          if (result.job_id !== job || result.task_id !== task) throw new Error('Page-copy result does not match this job.');
          if (result.status !== 'running') {
            Object.assign(state, { result, phase: 'saved', cancelling: false }); publish();
            if (finished) await finished();
            state.active = false; publish();
            break;
          }
          state.phase = result.phase; state.cancelling = result.cancelling === true; publish();
          await new Promise(resolve => setTimeout(resolve, 1000));
        }
      } catch (error) {
        state.active = false; state.error = `${error.message} Reopen this panel to recover the current task.`; publish();
      } finally { monitor = null; }
    })();
    return monitor;
  }
  function recover() {
    if (recovering || monitor) return;
    recovering = client('/api/job-source/pending', {}).then(value => {
      if (value.active) void follow(value.task_id, value.job_id, value.preview === true);
      else if (value.last) {Object.assign(state,{task:value.last.task_id,job:value.last.job_id,preview:value.last.kind==='recorded_preview',active:false,result:value.last});publish();}
    }).catch(error => { state.error = error.message; publish(); }).finally(() => { recovering = null; });
  }
  function create(job, dependencies) {
    client = dependencies.api; finished = dependencies.onFinished;
    const el = dependencies.element;
    const root = el('section', '', 'source-copy'); root.setAttribute('aria-label', 'Saved source page');
    const title = el('h4', 'Page copy'), hint = el('p', 'Save the original page and available resources with this job’s records.', 'footnote');
    const actions = el('div', '', 'source-copy-actions'), save = el('button', 'Save page copy'), stop = el('button', 'Stop current copy');
    save.type = stop.type = 'button'; stop.className = 'secondary';
    const progress = document.createElement('progress'); progress.setAttribute('aria-label', 'Page-copy stages'); progress.max=4;progress.value=0;
    const status = el('p', '', 'source-copy-status'); status.setAttribute('role', 'status'); status.setAttribute('aria-live', 'polite');
    const details = el('details', '', 'source-copy-result'), summary = el('summary', 'Capture details'), findings = el('div');
    details.append(summary, findings); actions.append(save, stop); root.append(title, hint, actions, progress, status, details);
    let renderedAttempt = null;
    function render() {
      const own = state.job === job.id;
      save.disabled = state.active;
      stop.hidden = !state.active; stop.disabled = state.cancelling || !state.task;
      progress.hidden = !state.active || !own;
      progress.value=({starting:0,tools:0,reading:1,resources:2,preview:2,rehearsal:2,saved:3})[state.phase]??0;
      save.textContent = own && state.result && state.result.kind !== 'recorded_preview' ? 'Save another copy' : 'Save page copy';
      status.className = 'source-copy-status';
      const showResult = own && !state.active && state.result?.status === 'complete';
      details.hidden = !showResult;
      if (state.active) status.textContent = (own ? (state.cancelling ? 'Stopping the background capture…' : (state.preview && state.phase === 'starting' ? 'Starting recorded preview…' : phases[state.phase] || 'Saving page copy…')) : 'Another job’s page copy is running. You can stop it here.') + (state.error ? ` ${state.error}` : '');
      else if (own && state.result) {
        status.textContent = description(state.result);
        if (state.result.status !== 'complete' || state.result.result?.needs_review || state.result.record_saved === false) status.classList.add('review-label');
      } else status.textContent = state.error || '';
      if (showResult && renderedAttempt !== state.result.attempt_id) {
        renderedAttempt = state.result.attempt_id;
        findings.replaceChildren();
        if (state.result.kind === 'recorded_preview') {
          summary.textContent = state.result.result?.preview?.rehearsal ? 'Offline autofill rehearsal' : 'Recorded page preview';
          findings.append(previewView(state.result.result?.preview, el, { api: client, jobId: job.id, attemptId: state.result.attempt_id, inline: true }));
          details.open = true;
          return;
        }
        summary.textContent = 'Capture details';
        const report = state.result.result?.report;
        const labels = { origin_not_declared: 'Some linked domains were outside this copy.', source_not_allowed: 'A source needs permission review.', robots_denied: 'A resource does not allow automated reading.', robots_unavailable: 'A resource’s reading rules could not be verified.', network: 'A resource could not be downloaded.', budget_exceeded: 'A capture limit was reached.', archive_rejected: 'A response could not be preserved reliably.', invalid_redirect: 'A redirect needs review.', redirect_limit: 'A redirect chain exceeded its limit.', redirect_loop: 'A redirect loop was found.' };
        const reasons = new Set((report?.records || []).filter(row => row.outcome?.state === 'needs_review').map(row => row.outcome.reason));
        for (const reason of reasons) findings.append(el('p', labels[reason] || 'A resource needs review.', 'footnote'));
        const rounds = report?.discovery?.rounds;
        if (Array.isArray(rounds)) findings.append(el('p', `${rounds.length} resource-discovery pass${rounds.length === 1 ? '' : 'es'} recorded.`, 'footnote'));
        findings.append(outlineView(report?.discovery?.form_outline,el,{api:client,jobId:job.id,attemptId:state.result.attempt_id,renderEvidence:dependencies.renderEvidence}));
        findings.append(el('p', `Saved under application record ${state.result.attempt_id}.`, 'footnote'));
      }
    }
    save.addEventListener('click', () => start(job.id));
    stop.addEventListener('click', async () => {
      if (!state.active || state.cancelling || !state.task) return;
      state.cancelling = true; state.error = null; publish();
      try { await client('/api/job-source/cancel', { task_id: state.task }); }
      catch (error) { state.cancelling = false; state.error = error.message; publish(); }
    });
    views.set(root, render); render(); recover();
    return root;
  }
  window.WaypointSourceCopies = { create, outlineView, previewAction, previewView };
})();
