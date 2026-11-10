// Invoked only by the contained native document qualification. All data fictional.
const fs = require('node:fs'), path = require('node:path'), crypto = require('node:crypto');
const assert = require('node:assert/strict');
const { chromium } = require('../../../target/layout-qa/node_modules/playwright-core');
const config = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const origin = new URL(config.url).origin, token = new URL(config.url).hash.slice(1), folder = config.output;
let browser, context;
const evidence = { complete: false, scope: 'Fictional saved profile/template through actual UI/model, PDF export and local vision; no employer interaction', documents: [], requests: [], errors: [], blocked: [] };
evidence.model_identity=config.model_identity;
async function response(route, body, extra = {}) {
  const r = await fetch(origin + route, { method: body ? 'POST' : 'GET', headers: { 'X-Workspace-Token': token, ...(body ? {'Content-Type':'application/json'} : {}), ...extra.headers }, body: body ? JSON.stringify(body) : undefined, ...extra });
  return r;
}
async function api(route, body) { const r = await response(route, body), value = await r.json(); assert(r.ok, value.error || route); return value; }
async function until(check, milliseconds = 180000) { const end = Date.now() + milliseconds; while (Date.now() < end) { const value = await check(); if (value) return value; await new Promise(resolve=>setTimeout(resolve,200)); } throw Error('Expected document state timed out'); }
(async()=>{
  try {
    const templateBytes = fs.readFileSync(config.template);
    const upload = await fetch(origin + '/api/documents/upload', {method:'POST',headers:{'X-Workspace-Token':token,'X-Document-Name':'Fictional-CV-template.docx'},body:templateBytes});
    const uploaded = await upload.json(); assert(upload.ok, JSON.stringify(uploaded));
    const original = await api('/api/state');
    const history = {version:1,employment:[{id:'a'.repeat(32),employer:'Fictional Signal Lab',title:'Systems engineer',start_date:'2019',end_date:'2023',description:'Built measurement tools in Rust and wrote operating guides.',reviewed:true}],education:[{id:'c'.repeat(32),institution:'Fictional Learning Institute',qualification:'Master of Data Science',start_date:'2025-09',current:true,description:'Studies in progress. No degree has been awarded.',reviewed:true}]};
    const profile = {first_name:'Avery',last_name:'Rowan',email:'avery@fixture.invalid',city:'Fictional City',auto_assess_ai:'off',document_instructions:'Use clear, concise language. Preserve unknowns and date precision.',career_history:JSON.stringify(history),cv_template_id:uploaded.id,cover_template_id:uploaded.id,_expected_profile_revision:original.profile_revision};
    await api('/api/profile', profile);
    const job = await api('/api/jobs/add',{url:'https://example.org/fictional-document-journey',title:'Research software engineer',employer:'Fictional Research Centre',description:'Build measurement tools in Rust, maintain reliable software and write operating guides.'});
    await api('/api/shortlist',{id:job.id});
    evidence.job_id=job.id;
    browser = await chromium.launch({headless:true,executablePath:config.browser});
    evidence.browser_version=browser.version();
    context = await browser.newContext({viewport:{width:1440,height:1100}});
    await context.route('**/*',route=>{if(new URL(route.request().url()).origin!==origin){evidence.blocked.push('External browser request');return route.abort();}return route.continue();});
    const page = await context.newPage(), pending = new Set();
    page.on('pageerror',error=>evidence.errors.push(error.message));
    page.on('response',r=>{
      if(new URL(r.url()).origin!==origin || !r.url().includes('/api/'))return;
      const task=(async()=>{const route=new URL(r.url()).pathname;if(r.request().method()==='POST')evidence.requests.push({path:route,status:r.status()});if(['/api/draft/status','/api/letter/export-status','/api/documents/visual-review-status'].includes(route)){const value=await r.json();if(value.status!=='running'){evidence[route]=value;fs.writeFileSync(path.join(folder,'progress.private.json'),JSON.stringify(evidence,null,2));}}})();
      pending.add(task);task.catch(error=>evidence.errors.push(error.message)).finally(()=>pending.delete(task));
    });
    assert.equal(crypto.createHash('sha256').update(await (await response('/app.js')).text()).digest('hex'),config.app_sha256);
    for(const kind of ['cv','cover_letter']) {
      delete evidence['/api/draft/status'];
      await page.goto(config.url,{waitUntil:'networkidle'});
      await page.getByRole('button',{name:kind==='cv'?'Draft CV':'Draft cover letter',exact:true}).click();
      const panel = page.locator('#packet'), label=kind==='cv'?'CV':'Cover letter';
      await until(async()=>{const terminal=evidence['/api/draft/status'];if(terminal?.status==='failed')throw Error('Draft failed: '+terminal.error);return panel.getByRole('textbox',{name:`Editable ${label} draft`,exact:true}).isVisible();});
      assert.equal(await panel.locator('.draft-source-text').count()>=2,true);
      const reviewed = await panel.getByRole('textbox',{name:`Editable ${label} draft`,exact:true}).inputValue();
      if(kind==='cover_letter'){const framing=evidence['/api/draft/status']?.draft?.framing;assert(framing?.opening&&framing?.closing,'Cover-letter opening and closing must be present');assert(reviewed.startsWith(framing.opening)&&reviewed.endsWith(framing.closing));}
      assert(reviewed.includes('Fictional Signal Lab') && reviewed.includes('2019') && reviewed.includes('2023'),'Expected employment disappeared');
      assert(!reviewed.includes('Fictional runtime') && !reviewed.includes('private runtime'),'Template content was imported as a candidate claim');
      await panel.getByRole('button',{name:`Save reviewed ${label} as PDF`,exact:true}).click();
      await panel.getByRole('button',{name:'Review PDF visually',exact:true}).waitFor({timeout:120000});
      await Promise.all([...pending]);
      const exported = evidence['/api/letter/export-status'];
      assert.equal(exported.kind,kind);assert.equal(exported.record_saved,true);assert.equal(exported.draft_provenance_saved,true);assert.equal(exported.text_check.status,'matches');
      const pdf = await response('/api/documents/download',{id:exported.id});assert(pdf.ok);const bytes=Buffer.from(await pdf.arrayBuffer());assert(bytes.subarray(0,5).equals(Buffer.from('%PDF-')));
      const item={id:exported.id,kind,name:exported.name,sha256:crypto.createHash('sha256').update(bytes).digest('hex'),reviewed};
      fs.writeFileSync(path.join(folder,`${kind}.reviewed.txt`),reviewed);
      await panel.getByRole('button',{name:'Review PDF visually',exact:true}).click();
      await panel.getByRole('button',{name:'Review PDF pages with local vision',exact:true}).click();
      const visual = await until(async()=>{const found=evidence['/api/documents/visual-review-status'];return found?.report?.document_sha256===item.sha256&&found;},180000);
      assert.equal(visual.record_saved,true);assert.equal(visual.report.complete,true);assert(visual.report.pages>=1);assert.equal(visual.report.pages_inspected,visual.report.pages);
      assert(visual.report.findings.every(row=>row.image_capture?.sha256?.length===64 && row.image_capture?.bytes>0),'Exact model page images must be archived');
      const currentReview=await api('/api/documents/job-check-review',{job_id:job.id,document_id:item.id,attempt_id:visual.attempt_id});assert.equal(currentReview.context_current,true);
      assert.match(visual.report.reference_date_utc,/^\d{4}-\d{2}-\d{2}$/);assert.equal(visual.report.source_coverage.checked_history_available,2);
      if(kind==='cover_letter')assert.equal(visual.report.findings.flatMap(row=>row.issues).filter(issue=>issue.category==='content').length,0,'Fictional cover letter has no labeled content conflict');
      item.visual=visual;
      if(kind==='cv') {
        const start=await api('/api/documents/ats-check-start',{id:item.id,job_id:job.id});
        item.readability=await until(async()=>{const v=await api('/api/documents/ats-check-status',{task_id:start.task_id});return v.status!=='running'&&v;});
        assert.equal(item.readability.status,'complete');
      }
      await page.screenshot({path:path.join(folder,`${kind}-visual-review.png`),fullPage:true});
      evidence.documents.push(item);
      fs.writeFileSync(path.join(folder,'progress.private.json'),JSON.stringify(evidence,null,2));
    }
    const saved=await api('/api/draft/list',{id:job.id});assert.equal(saved.drafts.length,2);
    const before=await api('/api/draft/read',{id:job.id,draft_id:saved.drafts[0].id});assert.equal(before.current,true);
    const current=await api('/api/state');history.employment[0].description='Corrected fictional history.';history.employment[0].reviewed=false;
    await api('/api/profile',{...profile,career_history:JSON.stringify(history),_expected_profile_revision:current.profile_revision});
    const stale=await api('/api/draft/read',{id:job.id,draft_id:saved.drafts[0].id});assert.equal(stale.current,false);
    for(const item of evidence.documents){const staleReview=await api('/api/documents/job-check-review',{job_id:job.id,document_id:item.id,attempt_id:item.visual.attempt_id});assert.equal(staleReview.context_current,false);}
    const refusal=await response('/api/letter/export-start',{id:job.id,kind:before.result.kind,text:evidence.documents.find(d=>d.kind===before.result.kind).reviewed,context_digest:before.result.digest,draft_id:saved.drafts[0].id,reviewed:true});assert.equal(refusal.status,400);evidence.stale_refusal=await refusal.json();
    const unchanged=await response('/api/documents/download',{id:uploaded.id});assert(Buffer.from(await unchanged.arrayBuffer()).equals(templateBytes),'Original template changed');
    evidence.history=await api('/api/attempts/list',{id:job.id});assert(evidence.history.attempts.length>=4);
    const final=await api('/api/state');assert.equal(final.jobs.find(row=>row.job.id===job.id).job.state,'shortlisted');
    await Promise.all([...pending]);assert.deepEqual(evidence.errors,[]);assert.deepEqual(evidence.blocked,[]);
    evidence.complete=true;
  } catch(error) {evidence.error=error.stack;process.exitCode=1;}
  finally {if(context)await context.close().catch(()=>{});if(browser)await browser.close().catch(()=>{});fs.writeFileSync(path.join(folder,'result.private.json'),JSON.stringify(evidence,null,2));console.log(JSON.stringify({complete:evidence.complete,error:evidence.error||null}));}
})();
