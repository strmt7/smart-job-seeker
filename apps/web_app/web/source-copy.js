/* Shared page-copy state survives job-list rerenders without duplicate polling. */
(() => {
  const views = new Map();
  const state = { task: null, job: null, active: false, phase: 'starting', cancelling: false, result: null, error: null };
  let client, monitor = null, recovering = null, finished;
  const phases = { starting: 'Starting page copy…', tools: 'Setting up background tools…', reading: 'Reading the original page…', resources: 'Collecting page resources…', saved: 'Finishing saved records…' };
  function publish() {
    for (const [node, render] of views) {
      if (node.isConnected) render(); else views.delete(node);
    }
  }
  function count(value) { return Number.isSafeInteger(value) && value >= 0 ? value : null; }
  function outlineView(outline, el) {
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
        list.append(row);
      }
    }
    render();root.append(list);
    if(outline.controls.length>12){const more=el('button',`Show all ${outline.controls.length} controls`,'secondary');more.type='button';more.setAttribute('aria-expanded','false');more.addEventListener('click',()=>{expanded=!expanded;render();more.textContent=expanded?'Show fewer controls':`Show all ${outline.controls.length} controls`;more.setAttribute('aria-expanded',String(expanded));});root.append(more);}
    if(outline.status==='partial')root.append(el('p','Some controls, labels or question context could not be retained. Review the original page.','footnote'));
    return root;
  }
  function description(value) {
    if (value.status === 'cancelled') return 'Stopped. Available files remain in application records.';
    if (value.status !== 'complete') return value.error || 'The page copy could not finish. Available files remain in application records.';
    const report = value.result?.report, resources = count(report?.recorded_resources), requests = count(report?.requests);
    const counts = resources === null || requests === null ? 'Check the saved record for capture details.' : `${resources} resource${resources === 1 ? '' : 's'} saved from ${requests} request${requests === 1 ? '' : 's'}.`;
    const review = value.result?.needs_review !== false || value.result?.archive_written !== true;
    const heading=value.result?.archive_written!==true?'Page copy incomplete. ':review?'Page copy saved with gaps. ':'Page copy saved. ';
    return `${heading}${counts} ${value.current_source === false ? 'The job URL changed; this copy belongs to the earlier source. ' : value.current_source!==true?'The current job URL could not be verified. ':''}${value.record_saved === false ? 'The record could not be finalized; check its folder. ' : ''}${value.review_flag_saved===false?'The job’s review flag could not be saved. ':''}Unopened sections and server checks are not verified.`;
  }
  async function follow(task, job) {
    if (monitor) return monitor;
    Object.assign(state, { task, job, active: true, result: null, error: null }); publish();
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
      if (value.active) void follow(value.task_id, value.job_id);
      else if (value.last) {Object.assign(state,{task:value.last.task_id,job:value.last.job_id,active:false,result:value.last});publish();}
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
    function render() {
      const own = state.job === job.id;
      save.disabled = state.active;
      stop.hidden = !state.active; stop.disabled = state.cancelling || !state.task;
      progress.hidden = !state.active || !own;
      progress.value=({starting:0,tools:0,reading:1,resources:2,saved:3})[state.phase]??0;
      save.textContent = own && state.result ? 'Save another copy' : 'Save page copy';
      status.className = 'source-copy-status';
      details.hidden = !(own && state.result?.status === 'complete');
      if (state.active) status.textContent = (own ? (state.cancelling ? 'Stopping the background capture…' : phases[state.phase] || 'Saving page copy…') : 'Another job’s page copy is running. You can stop it here.') + (state.error ? ` ${state.error}` : '');
      else if (own && state.result) {
        status.textContent = description(state.result);
        if (state.result.status !== 'complete' || state.result.result?.needs_review || state.result.record_saved === false) status.classList.add('review-label');
      } else status.textContent = state.error || '';
      if (own && state.result?.status === 'complete') {
        findings.replaceChildren();
        const report = state.result.result?.report;
        const labels = { origin_not_declared: 'Some linked domains were outside this copy.', source_not_allowed: 'A source needs permission review.', robots_denied: 'A resource does not allow automated reading.', robots_unavailable: 'A resource’s reading rules could not be verified.', network: 'A resource could not be downloaded.', budget_exceeded: 'A capture limit was reached.', archive_rejected: 'A response could not be preserved reliably.', invalid_redirect: 'A redirect needs review.', redirect_limit: 'A redirect chain exceeded its limit.', redirect_loop: 'A redirect loop was found.' };
        const reasons = new Set((report?.records || []).filter(row => row.outcome?.state === 'needs_review').map(row => row.outcome.reason));
        for (const reason of reasons) findings.append(el('p', labels[reason] || 'A resource needs review.', 'footnote'));
        const rounds = report?.discovery?.rounds;
        if (Array.isArray(rounds)) findings.append(el('p', `${rounds.length} resource-discovery pass${rounds.length === 1 ? '' : 'es'} recorded.`, 'footnote'));
        findings.append(outlineView(report?.discovery?.form_outline,el));
        findings.append(el('p', `Saved under application record ${state.result.attempt_id}.`, 'footnote'));
      }
    }
    save.addEventListener('click', async () => {
      if (state.active) return;
      Object.assign(state, { task:null,job: job.id, active: true, cancelling:false,phase: 'starting', result: null, error: null }); publish();
      try {
        const pending=await client('/api/job-source/pending',{});
        if(pending.active){await follow(pending.task_id,pending.job_id);return;}
        const value = await client('/api/job-source/start', { id: job.id });
        await follow(value.task_id, job.id);
      } catch (error) { state.active = false; state.error = error.message; publish(); recover(); }
    });
    stop.addEventListener('click', async () => {
      if (!state.active || state.cancelling || !state.task) return;
      state.cancelling = true; state.error = null; publish();
      try { await client('/api/job-source/cancel', { task_id: state.task }); }
      catch (error) { state.cancelling = false; state.error = error.message; publish(); }
    });
    views.set(root, render); render(); recover();
    return root;
  }
  window.WaypointSourceCopies = { create, outlineView };
})();
