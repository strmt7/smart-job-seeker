// Actual local-model template planning/editing in the fictional document harness.
const fs=require('node:fs'), path=require('node:path'), crypto=require('node:crypto'), assert=require('node:assert/strict');
const {chromium}=require('../../../target/layout-qa/node_modules/playwright-core');
const config=JSON.parse(fs.readFileSync(process.argv[2],'utf8')), origin=new URL(config.url).origin, token=new URL(config.url).hash.slice(1), folder=config.output;
const evidence={complete:false,scope:'Fictional styled DOCX through actual UI passage planning, local rewriting, saved DOCX/PDF and archived page review',documents:[],errors:[],blocked:[],responses:[]};
evidence.model_identity=config.model_identity;
const templateName=config.template_kind==='cover_letter'?'Fictional-styled-cover-letter.docx':'Fictional-styled-CV.docx';
let browser,context;
async function request(route,body,extra={}){return fetch(origin+route,{method:body?'POST':'GET',headers:{'X-Workspace-Token':token,...(body?{'Content-Type':'application/json'}:{}),...extra.headers},body:body?JSON.stringify(body):undefined,...extra});}
async function api(route,body){const response=await request(route,body),value=await response.json();assert(response.ok,value.error||route);return value;}
async function until(check,ms=240000){const deadline=Date.now()+ms;while(Date.now()<deadline){const value=await check();if(value)return value;await new Promise(resolve=>setTimeout(resolve,200));}throw Error('Template state timed out');}
(async()=>{try{
 const bytes=fs.readFileSync(config.template),upload=await fetch(origin+'/api/documents/upload',{method:'POST',headers:{'X-Workspace-Token':token,'X-Document-Name':templateName},body:bytes});
 const doc=await upload.json();assert(upload.ok,JSON.stringify(doc));evidence.original_template_sha256=crypto.createHash('sha256').update(bytes).digest('hex');
 const original=await api('/api/state');
 const profile={first_name:'Avery',last_name:'Rowan',email:'avery@fixture.invalid',auto_assess_ai:'off',document_instructions:'Remove wordiness from relevant experience descriptions. Preserve the original facts and past tense. Do not add claims, change names, dates or qualifications. Do not change ongoing studies into a completed degree.',_expected_profile_revision:original.profile_revision};
 await api('/api/profile',profile);
 const job=await api('/api/jobs/add',{url:'https://example.org/fictional-template-journey',title:'Research software engineer',employer:'Fictional Research Centre',description:'Build measurement tools in Rust, maintain software and write clear operating guides.'});
 await api('/api/shortlist',{id:job.id});evidence.job_id=job.id;
 browser=await chromium.launch({headless:true,executablePath:config.browser});evidence.browser_version=browser.version();
 context=await browser.newContext({viewport:{width:1440,height:1100}});
 await context.route('**/*',route=>{if(new URL(route.request().url()).origin!==origin){evidence.blocked.push('External browser request');return route.abort();}return route.continue();});
 const page=await context.newPage(),pending=new Set();page.on('pageerror',error=>evidence.errors.push(error.message));
 page.on('response',r=>{const route=new URL(r.url()).pathname;if(!['/api/documents/template-plan-status','/api/documents/template-suggest-status','/api/documents/edit-template-status','/api/documents/visual-review-status'].includes(route))return;const task=(async()=>{const value=await r.json();if(value.status!=='running'){evidence.responses.push({route,value,task_id:r.request().postDataJSON().task_id});if(value.status==='failed')evidence.terminal_error=value.error;fs.writeFileSync(path.join(folder,'progress.private.json'),JSON.stringify(evidence,null,2));}})();pending.add(task);task.catch(error=>evidence.errors.push(error.message)).finally(()=>pending.delete(task));});
 assert.equal(crypto.createHash('sha256').update(await (await request('/app.js')).text()).digest('hex'),config.app_sha256);
 await page.goto(config.url,{waitUntil:'networkidle'});await page.locator('nav[aria-label="Workspace sections"] a[href="#documents-section"]').click();
 await page.locator('.document-row').filter({hasText:templateName}).getByRole('button',{name:'Review text',exact:true}).click();
 const editor=page.locator('#document-preview');await editor.getByRole('combobox',{name:'Job for template suggestion'}).selectOption(job.id);
 await editor.getByRole('button',{name:'Suggest template changes',exact:true}).click();
 await until(async()=>{if(evidence.terminal_error)throw Error(evidence.terminal_error);const status=await editor.locator('.template-batch-suggestions [role=status]').textContent();if(/No passages were selected|Stopped after/.test(status))throw Error(status);return /suggestions ready/.test(status);});
 await Promise.all([...pending]);
 const planned=evidence.responses.find(r=>r.route==='/api/documents/template-plan-status').value;
 assert(planned.plan.passages.length>0,'Actual model must select a useful passage');assert(planned.plan.coverage.presented>0);assert.equal(planned.plan.editability,'original_passages_checked');
 const suggested=evidence.responses.filter(r=>r.route==='/api/documents/template-suggest-status').map(r=>r.value);
 assert(suggested.length>0&&suggested.length<=5);assert(suggested.some(r=>r.before!==r.suggestion.text),'Expected an actual rewrite');
 for(const row of suggested){assert.equal(row.suggestion.evidence_check.status,'completed');assert(!/\[\d+\]/.test(row.suggestion.text),'The fixture original contains no inline numeric citations');assert(row.suggestion.evidence_check.claims.every(c=>c.status!=='unsupported'),'Unsupported suggestion requires review');}
 const add=editor.getByRole('button',{name:'Add reviewed suggestion',exact:true});const count=await add.count();assert.equal(count,suggested.length);
 for(let i=0;i<count;i++)await add.first().click();assert.equal(await editor.locator('.template-edit-queue li').count(),count);
 evidence.geometry=[];
 for(const width of [320,768,1440]){await page.setViewportSize({width,height:1100});const row=await editor.locator('.template-batch-actions').evaluate(node=>({width:innerWidth,document:document.documentElement.scrollWidth,buttons:[...node.querySelectorAll('button')].map(b=>{const r=b.getBoundingClientRect();return {left:r.left,right:r.right,width:r.width};})}));assert(row.document<=width+1,'Horizontal page overflow');assert(row.buttons.every(b=>b.width>0&&b.left>=0&&b.right<=width+1),'Template action clipping');evidence.geometry.push(row);await page.screenshot({path:path.join(folder,`template-editor-${width}.png`),fullPage:true});}
 await editor.getByRole('button',{name:'Save queued DOCX and PDF changes',exact:true}).click();
 await editor.getByRole('button',{name:'Review edited PDF visually',exact:true}).waitFor({timeout:120000});await Promise.all([...pending]);
 const edited=evidence.responses.find(r=>r.route==='/api/documents/edit-template-status').value;evidence.edited=edited;
 assert.equal(edited.record_saved,true);assert.equal(edited.text_check.status,'matches');assert(edited.docx_id&&edited.pdf_id);
 await editor.getByRole('button',{name:'Review edited PDF visually',exact:true}).click();
 const packet=page.locator('#packet');await packet.getByRole('button',{name:'Review PDF pages with local vision',exact:true}).click();
 const visual=await until(()=>evidence.responses.find(r=>r.route==='/api/documents/visual-review-status')?.value);
 assert.equal(visual.status,'complete');assert.equal(visual.record_saved,true);assert.equal(visual.report.complete,true);assert.equal(visual.report.pages_inspected,visual.report.pages);for(const finding of visual.report.findings)if(finding.suggestions?.length||finding.issues.some(issue=>issue.category==='content'))assert.equal(finding.content_check?.status,'completed','All supplied content observations and optional advice need the bounded second check');
 const pdf=await request('/api/documents/download',{id:edited.pdf_id});assert(pdf.ok);const pdfBytes=Buffer.from(await pdf.arrayBuffer());assert(pdfBytes.subarray(0,5).equals(Buffer.from('%PDF-')));
 evidence.documents.push({id:edited.pdf_id,kind:'template',sha256:crypto.createHash('sha256').update(pdfBytes).digest('hex'),visual});
 await page.screenshot({path:path.join(folder,'template-visual-review.png'),fullPage:true});
 const originalAfter=await request('/api/documents/download',{id:doc.id});assert(Buffer.from(await originalAfter.arrayBuffer()).equals(bytes));
 evidence.history=await api('/api/attempts/list',{id:job.id});assert(evidence.history.attempts.length>=2);
 const state=await api('/api/state');await api('/api/profile',{...profile,document_instructions:'Changed preferences.',_expected_profile_revision:state.profile_revision});
 const stale=await api('/api/documents/template-plan-status',{task_id:evidence.responses.find(r=>r.route==='/api/documents/template-plan-status').task_id});
 assert.equal(stale.status,'failed');assert.match(stale.error,/changed/);evidence.stale_refusal=stale;
 evidence.original_unchanged=true;assert.equal(state.jobs.find(r=>r.job.id===job.id).job.state,'shortlisted');
 await Promise.all([...pending]);assert.deepEqual(evidence.errors,[]);assert.deepEqual(evidence.blocked,[]);evidence.complete=true;
}catch(error){evidence.error=error.stack;process.exitCode=1;if(context?.pages()[0])await context.pages()[0].screenshot({path:path.join(folder,'failure.png'),fullPage:true}).catch(()=>{});}finally{if(context)await context.close().catch(()=>{});if(browser)await browser.close().catch(()=>{});fs.writeFileSync(path.join(folder,'result.private.json'),JSON.stringify(evidence,null,2));console.log(JSON.stringify({complete:evidence.complete,error:evidence.error||null}));}})();
