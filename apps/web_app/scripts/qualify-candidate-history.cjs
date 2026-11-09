// Fictional-only, headless qualification against the compiled local app.
const {spawn}=require('node:child_process'), fs=require('node:fs'), path=require('node:path'), os=require('node:os'), assert=require('node:assert/strict'), crypto=require('node:crypto');
const {parseArgs}=require('node:util');
const {values:options}=parseArgs({options:{browser:{type:'string'},executable:{type:'string'}}});
const repo=path.resolve(__dirname,'../../..');
const {chromium}=require(path.join(repo,'target/layout-qa/node_modules/playwright-core'));
if(os.freemem()<6*1024**3)throw Error('Not enough available RAM for qualification');
const folder=fs.mkdtempSync(path.join(repo,'target/candidate-history-'));
const executable=options.executable||path.join(repo,'target/debug/waypoint_web.exe');
const browserExecutable=options.browser||'C:/Program Files/Google/Chrome/Application/chrome.exe';
const sources=['apps/web_app/src/main.rs','apps/web_app/src/candidate_history.rs','apps/web_app/src/history_advice.rs','apps/web_app/src/history_record_choice.rs','apps/web_app/web/history-mapping.js','apps/web_app/web/app.js','apps/web_app/web/index.html','apps/web_app/web/workspace-theme.css','apps/web_app/web/candidate-history.js','apps/web_app/scripts/qualify-candidate-history.cjs'];
const digest=file=>crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const evidence={complete:false,sources:Object.fromEntries(sources.map(file=>[file,digest(path.join(repo,file))])),executable_sha256:digest(executable),cases:[],errors:[],scope:'fictional profile, section advice interactions and actual browser zoom; no employer forms or model calls'};
const child=spawn(executable,[],{windowsHide:true,env:{...process.env,APPDATA:path.join(folder,'appdata'),WAYPOINT_WORKSPACE_DIR:path.join(folder,'workspace'),WAYPOINT_NO_BROWSER:'1',WAYPOINT_NO_TRAY:'1'},stdio:['ignore','pipe','pipe']});
let browser,context;
async function bounded(promise,label,ms=15000){let timer;try{return await Promise.race([promise,new Promise((_,reject)=>{timer=setTimeout(()=>reject(Error(label+' timed out')),ms);})]);}finally{clearTimeout(timer);}}
const started=Date.now();
(async()=>{
 try {
  const address=await bounded(new Promise((resolve,reject)=>{let output='';child.stdout.on('data',bytes=>{output+=bytes.toString();if(output.length>100000)return reject(Error('Startup output exceeded its limit'));const match=output.match(/http:\/\/127\.0\.0\.1:\d+\/#([a-f0-9]{64})/);if(match)resolve(match[0]);});child.once('error',reject);child.once('exit',()=>reject(Error('App exited during startup')));}), 'App startup',30000);
  child.stderr.on('data',()=>{});
  const origin=new URL(address).origin;
  // Reuse the layout qualifier's actual Chromium zoom controller, in an
  // owned throwaway profile. These flags belong only to this headless test.
  const extensionPath=path.join(folder,'extension'),browserProfile=path.join(folder,'browser');
  fs.mkdirSync(extensionPath);
  fs.writeFileSync(path.join(extensionPath,'manifest.json'),JSON.stringify({manifest_version:3,name:'Smart Jobseeker history layout qualification',version:'1.0',permissions:['tabs']}));
  fs.writeFileSync(path.join(extensionPath,'control.html'),'<!doctype html><title>History layout controller</title>');
  context=await chromium.launchPersistentContext(browserProfile,{executablePath:browserExecutable,headless:true,viewport:{width:1440,height:1000},args:['--disable-gpu','--enable-unsafe-extension-debugging','--remote-debugging-port=0'],ignoreDefaultArgs:['--disable-extensions']});
  const port=fs.readFileSync(path.join(browserProfile,'DevToolsActivePort'),'utf8').split('\n')[0];
  browser=await chromium.connectOverCDP('http://127.0.0.1:'+port);
  evidence.browser_version=browser.version();
  const cdp=await browser.newBrowserCDPSession(),extension=await cdp.send('Extensions.loadUnpacked',{path:extensionPath});
  await context.route('**/*',route=>{const url=new URL(route.request().url());return url.origin===origin||url.protocol==='chrome-extension:'?route.continue():route.abort();});
  async function open(){const page=await context.newPage();page.setDefaultTimeout(12000);page.on('pageerror',error=>evidence.errors.push(error.message));await page.goto(address);await page.waitForFunction(()=>typeof profileEditRevision==='string');await page.locator('#profile-editor > summary').click();await page.getByRole('button',{name:'Add employment',exact:true}).waitFor();return page;}
  const page=await open();
  await page.getByRole('button',{name:'Add employment',exact:true}).click();
  const entry=page.locator('.history-section').first().locator('.history-entry').first();
  await entry.getByLabel('Employer',{exact:true}).fill('Fictional & Co <literal>');
  await entry.getByLabel('Job title',{exact:true}).fill('Engineer');
  await entry.getByLabel('Start date',{exact:true}).fill('2020');
  await entry.getByLabel('End date',{exact:true}).fill('2023-06');
  await entry.getByLabel('Ongoing',{exact:true}).click();
  assert.equal(await entry.getByLabel('Ongoing',{exact:true}).isChecked(),false);
  assert.equal(await entry.getByLabel('End date',{exact:true}).inputValue(),'2023-06');
  assert.equal(await entry.getByLabel('End date',{exact:true}).evaluate(input=>input.validity.valid),true,'refused ongoing toggle must not invalidate the retained end date');
  await entry.getByLabel('End date',{exact:true}).fill('');
  await entry.getByLabel('Ongoing',{exact:true}).check();
  assert.equal(await entry.getByLabel('End date',{exact:true}).isDisabled(),true);
  await entry.getByLabel('Responsibilities and achievements',{exact:true}).fill('Built fictional test systems.');
  await entry.getByLabel('I have checked these details',{exact:true}).check();
  await entry.getByLabel('Job title',{exact:true}).fill('Systems engineer');
  assert.equal(await entry.getByLabel('I have checked these details',{exact:true}).isChecked(),false);
  await entry.getByLabel('I have checked these details',{exact:true}).check();
  await page.getByRole('button',{name:'Add education',exact:true}).focus();
  await page.getByRole('button',{name:'Add education',exact:true}).press('Enter');
  const education=page.locator('.history-section').nth(1).locator('.history-entry');
  await education.getByLabel('School or university',{exact:true}).fill('Fictional University');
  await education.getByLabel('Qualification',{exact:true}).fill('MSc');
  await education.getByLabel('Start date',{exact:true}).fill('2014');
  await education.getByLabel('End date',{exact:true}).fill('2018');
  await education.getByLabel('I have checked these details',{exact:true}).check();
  await page.getByRole('button',{name:'Save personal details',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('#profile-editor').open===false);
  await page.reload();await page.waitForFunction(()=>typeof profileEditRevision==='string');await page.locator('#profile-editor > summary').click();
  let saved=await page.locator('#career-history-value').inputValue();
  assert.equal(JSON.parse(saved).employment[0].start_date,'2020');
  assert.equal(JSON.parse(saved).employment[0].reviewed,true);
  assert.equal(JSON.parse(saved).education[0].qualification,'MSc');
  assert.equal(await page.locator('#candidate-history literal').count(),0);
  evidence.cases.push('date precision, ongoing refusal, edit invalidation, checked save and reload, literal text');
  await page.getByRole('button',{name:'Add employment',exact:true}).click();
  const second=page.locator('.history-section').first().locator('.history-entry').nth(1);
  await second.getByLabel('Employer',{exact:true}).fill('Earlier Fictional Co');
  await second.getByLabel('Job title',{exact:true}).fill('Earlier role');
  await second.getByRole('button',{name:'Move up',exact:true}).click();
  assert.equal(JSON.parse(await page.locator('#career-history-value').inputValue()).employment[0].employer,'Earlier Fictional Co');
  const moved=page.locator('.history-section').first().locator('.history-entry').first();
  await moved.getByRole('button',{name:'Remove employment entry 1',exact:true}).click();
  assert.equal(await page.locator('.history-section').first().locator('.history-entry').count(),2);
  await moved.getByRole('button',{name:'Confirm removal of employment entry 1',exact:true}).click();
  assert.equal(await page.locator('.history-section').first().locator('.history-entry').count(),1);
  evidence.cases.push('stable entry identity, reordering and two-step removal');
  const other=await open();
  await entry.getByLabel('Employer',{exact:true}).fill('My unsaved fictional edit');
  await other.locator('.history-section').first().getByLabel('Job title',{exact:true}).fill('Other saved edit');
  await other.getByRole('button',{name:'Save personal details',exact:true}).click();
  await other.waitForFunction(()=>document.querySelector('#profile-editor').open===false);
  await page.getByRole('button',{name:'Save personal details',exact:true}).click();
  await page.getByRole('button',{name:'Compare and rebase edits',exact:true}).waitFor();
  assert.equal(await entry.getByLabel('Employer',{exact:true}).inputValue(),'My unsaved fictional edit');
  await page.getByRole('button',{name:'Compare and rebase edits',exact:true}).click();
  assert.equal(await entry.getByLabel('Employer',{exact:true}).inputValue(),'My unsaved fictional edit');
  assert.match(await page.locator('[data-profile-rebase]').innerText(),/conflict/i);
  evidence.cases.push('concurrent save refusal and explicit history-conflict rebase preserves draft');
  await other.close();
  for(const width of [320,390,768,1440,1920]) {
   await page.setViewportSize({width,height:1000});
   await page.locator('#candidate-history').scrollIntoViewIfNeeded();
   await page.waitForTimeout(100);
   const overflow=await page.evaluate(()=>{const host=document.querySelector('#candidate-history'),bounds=host.getBoundingClientRect();return {document:document.documentElement.scrollWidth>innerWidth+1,history:host.scrollWidth>host.clientWidth+1,fields:[...host.querySelectorAll('input:not([type=checkbox]),textarea,button')].filter(el=>{const r=el.getBoundingClientRect();return r.width&&((r.right>bounds.right+2)||(r.left<bounds.left-2));}).map(el=>el.tagName)};});
   assert.deepEqual(overflow,{document:false,history:false,fields:[]},`overflow at ${width}`);
   evidence.cases.push(`populated history layout at ${width}px`);
   if(width===390||width===1440){await page.evaluate(()=>{document.querySelector('#message').classList.remove('visible');document.querySelector('#candidate-history').scrollIntoView({block:'start',behavior:'instant'});});await page.screenshot({path:path.join(folder,`history-${width}.png`)});}
  }
  await page.setViewportSize({width:1440,height:1000});
  const description=page.locator('.history-section').first().getByLabel('Responsibilities and achievements',{exact:true});
  await description.scrollIntoViewIfNeeded();
  const beforeResize=await description.boundingBox();
  await page.mouse.move(beforeResize.x+beforeResize.width-3,beforeResize.y+beforeResize.height-3);
  await page.mouse.down();await page.mouse.move(beforeResize.x+beforeResize.width-3,beforeResize.y+beforeResize.height+60,{steps:8});await page.mouse.up();
  const afterResize=await description.boundingBox();
  assert.ok(afterResize.height>=beforeResize.height+20&&afterResize.height<=360,'description mouse resizing stayed bounded');
  evidence.cases.push('native mouse-drag description resizing with bounded height');
  const inspected=await page.evaluate(()=>{const input=document.createElement('textarea'),box=profileValuePicker({career_history:document.querySelector('#career-history-value').value},input,{label:'Previous school',field_type:'text',sensitive:false});document.querySelector('#candidate-history').append(box);const select=box.querySelector('select');const option=[...select.options].find(item=>item.value.endsWith(':institution'));select.value=option.value;box.querySelector('button').click();return input.value;});
  assert.equal(inspected,'Fictional University');
  evidence.cases.push('explicit checked education value selection uses exact saved text');
  const historyCalls=[],historyStarts=[];
  await page.route('**/api/form/history-advice-*',async route=>{
   const name=new URL(route.request().url()).pathname;historyCalls.push(name);if(name.endsWith('-start'))historyStarts.push(route.request().postDataJSON());
   const response=name.endsWith('-start')?{task_id:'fictional-layout'}:{status:'complete',advice:{entry_label:'Engineer at Fictional Co',selection:{basis:'current',evidence:'Current employment'},proposals:[{selector:'#history-employer',label:'Employer',value:{text:'Fictional Co'}},{selector:'#history-month',label:'Start month',value:{date:'2020-06'}}],manual_review:[]}};
   await route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(response)});
  });
  await page.evaluate(()=>{
   const host=document.createElement('section');host.id='history-advice-qualification';
   const answers=[['history-employer','text'],['history-month','month'],['history-other','text']].map(([id,type])=>{const label=document.createElement('label');label.textContent=id==='history-employer'?'Employer':'Start month';const input=document.createElement('input');input.id=id;input.type=type;label.append(input);host.append(label);return {selector:'#'+id,input,kind:type};});
   const section=(id,label)=>({sections:[{id,label,inferred:false}]});
   const profile={career_history:JSON.stringify({version:1,employment:[{id:'c'.repeat(32),employer:'Fictional Co',title:'Engineer',start_date:'2020-06',end_date:'2023',description:'Fictional evidence only',ongoing:false,reviewed:true}],education:[]})};
   host.append(HistoryMapping.control({session_id:'fictional-layout',fields:[{selector:'#history-employer',label:'Employer',field_type:'text',sensitive:false,context:section('current','Current employment')},{selector:'#history-month',label:'Start month',field_type:'month',sensitive:false,context:section('current','Current employment')},{selector:'#history-other',label:'Employer',field_type:'text',sensitive:false,context:section('previous','Previous employment')}]},answers,profile));
   document.querySelector('#workspace-pane').append(host);
  });
  const advice=page.locator('#history-advice-qualification');
  await advice.locator('summary').click();
  assert.equal(await advice.getByLabel('History entry',{exact:true}).inputValue(),'auto');
  await advice.getByLabel('Form section',{exact:true}).selectOption('current');
  assert.equal(await advice.locator('fieldset input:checked').count(),2);
  assert.equal(await advice.locator('fieldset label').filter({hasText:'Employer · Section 2'}).count(),1);
  await advice.getByRole('button',{name:'Suggest answers',exact:true}).click();
  await advice.getByRole('button',{name:'Use selected suggestions',exact:true}).waitFor();
  assert.equal(await advice.locator('#history-employer').inputValue(),'');
  assert.equal(await advice.locator('.history-proposal input:checked').count(),0);
  for(const width of [320,390,768,1440,1920]) {
   await page.setViewportSize({width,height:1000});await advice.scrollIntoViewIfNeeded();
   const overflow=await advice.evaluate(host=>({document:document.documentElement.scrollWidth>innerWidth+1,host:host.scrollWidth>host.clientWidth+1}));
   assert.deepEqual(overflow,{document:false,host:false},`history advice overflow at ${width}`);
   evidence.cases.push(`history advice rendered at ${width}px with fictional intercepted responses`);
   if(width===390||width===1440)await page.screenshot({path:path.join(folder,`history-advice-${width}.png`)});
  }
  const controller=await context.newPage();
  await controller.goto('chrome-extension://'+extension.id+'/control.html');
  await page.bringToFront(); // A headless tab only; never a desktop interaction.
  async function zoom(factor){
   const actual=await bounded(controller.evaluate(async({origin,factor})=>{const tabs=await chrome.tabs.query({url:origin+'/*'});if(tabs.length!==1)throw Error('History application tab is ambiguous');await chrome.tabs.setZoomSettings(tabs[0].id,{mode:'automatic',scope:'per-tab'});await chrome.tabs.setZoom(tabs[0].id,factor);return chrome.tabs.getZoom(tabs[0].id);},{origin,factor}),'Browser zoom');
   assert.ok(Math.abs(actual-factor)<0.01,'requested browser zoom applied');
  }
  for(const width of [1280,1920]){
   await page.setViewportSize({width,height:1000});
   for(const factor of [0.8,1,1.25,1.5,2,4]){
    await zoom(factor);await page.bringToFront();await page.waitForTimeout(80);
    for(const control of [advice.getByLabel('Form section',{exact:true}),advice.getByRole('button',{name:'Suggest answers',exact:true})]){
     await control.scrollIntoViewIfNeeded();
     await control.evaluate(el=>el.scrollIntoView({block:'center',behavior:'instant'}));
     await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
     const hit=await control.evaluate(el=>{const r=el.getBoundingClientRect(),x=r.left+r.width/2,y=r.top+r.height/2;const hit=document.elementFromPoint(x,y);return {visible:r.width>0&&r.height>0&&x>=0&&y>=0&&x<innerWidth&&y<innerHeight,unobstructed:hit===el||el.contains(hit)};});
     assert.deepEqual(hit,{visible:true,unobstructed:true},`history control at ${width}px and ${factor} zoom`);
    }
    const overflow=await advice.evaluate(host=>({document:document.documentElement.scrollWidth>innerWidth+1,host:host.scrollWidth>host.clientWidth+1}));
    assert.deepEqual(overflow,{document:false,host:false},`history advice overflow at ${width}px and ${factor} zoom`);
    evidence.cases.push(`history advice actual browser zoom ${factor} at ${width}px`);
    if(width===1280&&factor===4){
     const session=await context.newCDPSession(page);
     const metrics=await session.send('Page.getLayoutMetrics');
     fs.writeFileSync(path.join(folder,'zoom-metrics.private.json'),JSON.stringify({metrics,dom:await page.evaluate(()=>({width:innerWidth,height:innerHeight,dpr:devicePixelRatio,scrollX,scrollY,visibility:document.visibilityState}))},null,2));
     // Playwright's viewport adjustment during screenshot capture produces a
     // blank image at actual tab zoom. Capture the existing compositor surface
     // directly, without altering its metrics or the application's styles.
     const capture=await session.send('Page.captureScreenshot',{format:'png',fromSurface:true,captureBeyondViewport:false});
     fs.writeFileSync(path.join(folder,'history-advice-zoom-400.png'),Buffer.from(capture.data,'base64'));
     await session.detach();
    }
   }
  }
  await zoom(1);await controller.close();await page.setViewportSize({width:1440,height:1000});
  for(const check of await advice.locator('.history-proposal input[type=checkbox]').all())await check.check();
  await advice.getByRole('button',{name:'Use selected suggestions',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('#history-employer').value==='Fictional Co');
  assert.equal(await advice.locator('#history-employer').inputValue(),'Fictional Co');
  assert.equal(await advice.locator('#history-month').inputValue(),'2020-06');
  assert.equal(await advice.locator('#history-other').inputValue(),'');
  assert.equal(historyStarts.length,1);
  assert.equal(historyStarts[0].entry_id,'auto');
  assert.deepEqual(historyStarts[0].selectors,['#history-employer','#history-month']);
  assert.match(await advice.locator('.history-mapping-result').innerText(),/Confirmed current entry/);
  assert.ok(historyCalls.every(name=>name.endsWith('-start')||name.endsWith('-status')));
  evidence.cases.push('automatic choice, section picker, duplicate-label distinction and unchecked proposals update only local answer editors; no fill request');
  await page.evaluate(()=>CandidateHistory.load({version:2}));
  assert.equal(await page.locator('#career-history-value').inputValue(),'{"version":2}');
  assert.match(await page.locator('#candidate-history').innerText(),/could not be read/);
  await page.getByRole('button',{name:'Save personal details',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('#message').textContent.includes('invalid structure'));
  await page.reload();await page.waitForFunction(()=>typeof profileEditRevision==='string');
  assert.equal(JSON.parse(await page.locator('#career-history-value').inputValue()).employment[0].title,'Other saved edit');
  evidence.cases.push('unreadable loaded data stays explicit and cannot silently clear saved history');
  assert.equal(evidence.errors.length,0);
  assert.deepEqual(Object.fromEntries(sources.map(file=>[file,digest(path.join(repo,file))])),evidence.sources);
  evidence.complete=true;
 } catch(error) {evidence.failure=error.message;throw error;}
 finally {
  try {
   if(context)await bounded(context.close(),'Browser context cleanup');
   if(browser?.isConnected())await bounded(browser.close(),'Browser cleanup');
  } catch(error) {evidence.complete=false;throw error;}
  finally {
   try {if(child.exitCode===null){child.kill();await bounded(new Promise(resolve=>child.once('exit',resolve)),'App cleanup');}}
   catch(error) {evidence.complete=false;throw error;}
   finally {
    evidence.seconds=+((Date.now()-started)/1000).toFixed(3);
    fs.writeFileSync(path.join(folder,'result.private.json'),JSON.stringify(evidence,null,2));
   }
  }
 }
 console.log('Headless career history qualification passed: '+folder);
})().catch(error=>{console.error(error.stack);process.exitCode=1;});
