// Development-only qualification. All browsers are headless and profiles are disposable.
const { spawn } = require('node:child_process'),
  fs = require('node:fs'),
  path = require('node:path');
const { parseArgs } = require('node:util'),
  os = require('node:os'),
  crypto = require('node:crypto');
const { values: options } = parseArgs({
  options: {
    quick: { type: 'boolean' },
    'source-assets': { type: 'boolean' },
    browser: { type: 'string' },
    executable: { type: 'string' },
    help: { type: 'boolean' },
  },
});
if (options.help) {
  console.log(
    'Headless layout qualification using fictional data and actual browser zoom.\nOptions: --quick --source-assets --browser <Chromium exe> --executable <app exe>\nInstall dev dependency: npm install --prefix target/layout-qa --no-save --ignore-scripts playwright-core@1.65.0',
  );
  process.exit(0);
}
if(process.platform==='win32' && process.env.WAYPOINT_LAYOUT_CONTAINED!=='1')throw Error('Run python -B tools/qualify_layout.py so all layout processes are owned and bounded');
const repo = path.resolve(__dirname, '../../..');
const { chromium } = require(path.join(repo, 'target/layout-qa/node_modules/playwright-core'));
if (os.freemem() < Math.min(6 * 1024 ** 3, Math.max(2 * 1024 ** 3, os.totalmem() * 0.15)))
  throw Error('Not enough available RAM for layout qualification');
const browserExecutable =
  options.browser || 'C:/Program Files/Google/Chrome/Application/chrome.exe';
const executable = options.executable || path.join(repo, 'target/debug/waypoint_web.exe');
if (!fs.existsSync(executable) || !fs.existsSync(browserExecutable))
  throw Error('Build the app and select an installed Chromium executable first');
const root = repo,
  scratch = fs.mkdtempSync(path.join(root, 'target', 'layout-matrix-'));
const child = spawn(executable, [], {
  env: {
    ...process.env,
    APPDATA: path.join(scratch, 'appdata'),
    WAYPOINT_WORKSPACE_DIR: path.join(scratch, 'workspace'),
    WAYPOINT_NO_BROWSER: '1',
    WAYPOINT_NO_TRAY: '1',
  },
  windowsHide: true,
  stdio: ['ignore', 'pipe', 'pipe'],
});
process.once('exit', () => child.kill());
fs.writeFileSync(
  path.join(scratch, 'process-owner.json'),
  JSON.stringify({
    appPid: child.pid,
    controllerPid: process.pid,
    executable: path.resolve(executable),
    startedAt: Date.now(),
  }),
);
console.log('Layout evidence: '+scratch);
let context, browser, page, build=null, phase='startup';
const errors=[],failures=[],cases=[];
async function visibleBox(locator,label){
  phase=label;
  await bounded(locator.waitFor({state:'visible',timeout:10000}),label+' visibility',11000);
  await bounded(locator.scrollIntoViewIfNeeded({timeout:10000}),label+' stability',11000);
  const box=await locator.boundingBox();
  if(!box||!['x','y','width','height'].every(key=>Number.isFinite(box[key]))||box.width<=0||box.height<=0)throw Error(label+' has no usable visible geometry');
  return box;
}

async function bounded(operation, label, milliseconds = 15000) {
  let timer;
  try {
    return await Promise.race([
      operation,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(Error(label + ' timed out')), milliseconds);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}
(async () => {
  let completed=false,fatal=null;
  try {
    const address = await new Promise((resolve, reject) => {
      let out = '';
      const cleanup = () => {
        clearTimeout(timer);
        child.stdout.off('data', onData);
        child.off('exit', onExit);
        child.off('error', onError);
        child.stdout.resume();
      };
      const onExit = () => { cleanup(); reject(Error('server exited')); };
      const onError = error => { cleanup(); reject(error); };
      const timer = setTimeout(() => { cleanup(); reject(Error('startup timeout')); }, 30000);
      const onData = buffer => {
        out += buffer;
        if (out.length > 65536) {
          cleanup();
          reject(Error('startup output exceeded its budget'));
          return;
        }
        const match = out.match(/http:\/\/127\.0\.0\.1:\d+\/#([a-f0-9]{64})/);
        if (match) {
          cleanup();
          resolve(match[0]);
        }
      };
      child.stdout.on('data', onData);
      child.stderr.resume();
      child.once('exit', onExit);
      child.once('error', onError);
    });
    const origin = new URL(address).origin,
      extensionPath = path.join(scratch, 'extension'),
      profile = path.join(scratch, 'browser');
    fs.mkdirSync(extensionPath);
    fs.writeFileSync(
      path.join(extensionPath, 'manifest.json'),
      JSON.stringify({
        manifest_version: 3,
        name: 'Smart Jobseeker layout qualification',
        version: '1.0',
        permissions: ['tabs'],
      }),
    );
    fs.writeFileSync(
      path.join(extensionPath, 'control.html'),
      '<!doctype html><title>Layout test controller</title>',
    );
    context = await chromium.launchPersistentContext(profile, {
      viewport: { width: 1440, height: 900 },
      executablePath: browserExecutable,
      headless: true,
      args: ['--enable-unsafe-extension-debugging', '--remote-debugging-port=0'],
      ignoreDefaultArgs: ['--disable-extensions'],
    });
    const port = fs.readFileSync(path.join(profile, 'DevToolsActivePort'), 'utf8').split('\n')[0];
    browser = await chromium.connectOverCDP('http://127.0.0.1:' + port);
    const cdp = await browser.newBrowserCDPSession(),
      extension = await cdp.send('Extensions.loadUnpacked', { path: extensionPath });
    page = await context.newPage();
    page.on('pageerror', (error) => errors.push(error.message));
    const pdf = {
      id: 'fictional-pdf',
      name: 'Fictional engineering CV with a deliberately long document filename.pdf',
      size: 24000,
    };
    const jobs = [
      [
        'Senior Research Engineer — Applied Systems and Platform Reliability',
        'Fictional Centre for Applied Engineering and Environmental Research',
      ],
      ['Product Designer', 'Riverbend Studio'],
      ['Operations Coordinator', 'Northstar Labs'],
    ].map(([title, employer], i) => ({
      job: {
        id: 'fictional-' + i,
        title,
        employer,
        location: 'Berlin · Hybrid',
        state: 'discovered',
        source: 'Fictional layout fixture',
      },
      score: 0,
      excluded_reasons: [],
      unknown_constraints: [],
    }));
    const png = fs
      .readFileSync(path.join(root, 'apps/web_app/tests/fictional-styled-document.png'))
      .toString('base64');
    await context.route('**/*', async (route) => {
      try {
        const u = new URL(route.request().url());
        if (u.protocol === 'chrome-extension:') return route.continue();
        if (u.origin !== origin) return route.abort();
        if (u.pathname === '/api/state') {
          const response = await route.fetch(),
            data = await response.json();
          data.jobs = jobs;
          data.documents = [
            pdf,
            { id: 'fictional-docx', name: 'Fictional cover letter.docx', size: 17000 },
          ];
          data.profile = { ...data.profile, cv_template_id: pdf.id,
            career_history: JSON.stringify({version:1,employment:[{id:'a'.repeat(32),employer:'Fictional Centre for Applied Engineering and Environmental Research',title:'Systems engineer with a deliberately long title for layout qualification',location:'Fictional City',start_date:'2020',end_date:'',current:true,description:'Built fictional systems. '.repeat(8),reviewed:true}],education:[{id:'b'.repeat(32),institution:'Fictional University',qualification:'MSc',start_date:'2014',end_date:'2018',reviewed:false}]}) };
          return route.fulfill({ response, json: data });
        }
        if (u.pathname === '/api/documents/preview-start')
          return route.fulfill({ json: { task_id: 'fictional-preview' } });
        if (u.pathname === '/api/documents/preview-status')
          return route.fulfill({
            json: { status: 'complete', page: { page: 0, pages: 1, image: png } },
          });
        const file = u.pathname === '/' ? 'index.html' : u.pathname.slice(1);
        if (
          options['source-assets'] &&
          ['index.html', 'app.js', 'layout.js', 'candidate-history.js', 'history-mapping.js', 'style.css', 'workspace-theme.css'].includes(file)
        )
          return route.fulfill({
            path: path.join(root, 'apps/web_app/web', file),
            contentType: file.endsWith('.css')
              ? 'text/css'
              : file.endsWith('.js')
                ? 'text/javascript'
                : 'text/html',
          });
        return route.continue();
      } catch (error) {
        errors.push('Request interception failed: ' + error.message);
        await route.abort().catch(() => {});
      }
    });
    await page.goto(address, { waitUntil: 'networkidle' });
    if ((await page.title()) !== 'Smart Jobseeker') throw Error('Unexpected application title');
    const controller = await context.newPage();
    await controller.goto('chrome-extension://' + extension.id + '/control.html');
    await page.bringToFront();
    await page.locator('#open-job-search').click();
    if (
      !(await page
        .locator('#web-search input[name="query"]')
        .evaluate((el) => document.activeElement === el))
    )
      throw Error('Find jobs did not open the wider-web search');
    await page.locator('#web-search input[name="query"]').evaluate((el) => {
      for (let parent = el.parentElement; parent; parent = parent.parentElement)
        if (parent.tagName === 'DETAILS') parent.open = false;
    });

    async function zoom(factor) {
      const actual = await bounded(
        controller.evaluate(
          async ({ origin, factor }) => {
            const [tab] = await chrome.tabs.query({ url: origin + '/*' });
            if (!tab) throw Error('Application tab missing');
            await chrome.tabs.setZoomSettings(tab.id, { mode: 'automatic', scope: 'per-tab' });
            await chrome.tabs.setZoom(tab.id, factor);
            return chrome.tabs.getZoom(tab.id);
          },
          { origin, factor },
        ),
        'Browser zoom',
      );
      if (Math.abs(actual - factor) > 0.01) throw Error('Browser zoom did not apply');
    }
    async function settle() {
      await bounded(
        page.evaluate(
          () =>
            new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))),
        ),
        'Headless layout settling',
        5000,
      );
    }
    async function audit(label, headingSelector = null) {
      phase=label;
      await settle();
      const result = await page.evaluate((headingSelector) => {
        const dialog = document.querySelector('dialog[open]'),
          box = dialog?.getBoundingClientRect();
        const view = document.querySelector('aside').dataset.view;
        const section = {
          profile: '#profile-section',
          documents: '#documents-section',
          settings: '#settings-section',
          applications: '#applications-section',
        }[view];
        const heading = document.querySelector(headingSelector || section + '>h2')?.getBoundingClientRect();
        const tabs = document
          .querySelector('nav[aria-label="Workspace sections"]')
          .getBoundingClientRect();
        const headingClipped =
          !dialog &&
          innerWidth > 850 &&
          heading &&
          heading.bottom > tabs.top &&
          heading.top < tabs.bottom - 1;

        return {
          width: innerWidth,
          height: innerHeight,
          ratio: devicePixelRatio,
          overflow: document.documentElement.scrollWidth > innerWidth + 1,
          historyOverflow: view === 'profile' && (()=>{const history=document.querySelector('#candidate-history');return Boolean(history&&history.scrollWidth>history.clientWidth+1);})(),
          headingClipped: Boolean(headingClipped),
          dialogOutside: Boolean(
            box &&
            (box.x < -1 ||
              box.y < -1 ||
              box.right > innerWidth + 1 ||
              box.bottom > innerHeight + 1),
          ),
          offenders: [...document.querySelectorAll('body *')]
            .filter((el) => {
              const box = el.getBoundingClientRect();
              return (
                box.width > 0 &&
                box.height > 0 &&
                box.right > innerWidth + 2 &&
                getComputedStyle(el).position !== 'absolute'
              );
            })
            .slice(0, 8)
            .map((el) => ({
              tag: el.tagName,
              id: el.id,
              className: typeof el.className === 'string' ? el.className : '',
              right: Math.round(el.getBoundingClientRect().right),
            })),
        };
      }, headingSelector);
      cases.push({ label, ...result });
      fs.writeFileSync(
        path.join(scratch, 'progress.json'),
        JSON.stringify({ cases: cases.length, latest: label }),
      );
      if (result.overflow || result.historyOverflow || result.dialogOutside || result.headingClipped)
        failures.push({ label, ...result });
      return result;
    }
    for (const [width, height] of [
      [320, 568],
      [390, 844],
      [568, 320],
      [768, 1024],
      [850, 600],
      [851, 600],
      [1024, 768],
      [1280, 960],
      [1440, 900],
      [1920, 1080],
      [2560, 1440],
      [3840, 2160],
    ]) {
      await page.setViewportSize({ width, height });
      for (const factor of [0.8, 1, 1.25, 1.5, 2, 4]) {
        if (width / factor < 319 || height / factor < 239 || options.quick) continue;
        await zoom(factor);
        for (const view of ['profile', 'documents', 'settings', 'applications']) {
          await page.evaluate((view) => {
            setWorkspaceView(view);
            document.querySelector('#profile-editor').open = true;
          }, view);
          await audit(`${width}x${height}@${factor}:${view}`);
          if (
            view === 'settings' &&
            ((width === 3840 && factor === 1) || (width === 1280 && factor === 4))
          ) {
            await page.screenshot({
              path: path.join(scratch, `settings-${width}-zoom-${factor}.png`),
            });
          }
        }
        await page.locator('#open-license').click();
        await audit(`${width}x${height}@${factor}:license`);
        await page.locator('#close-license').click();
      }
    }
    await page.setViewportSize({ width: 1440, height: 900 });
    await zoom(1);
    await page.bringToFront();
    await page.evaluate(() => setWorkspaceView('documents'));
    await settle();
    const divider = page.locator('main>.resize-divider');
    let box = await visibleBox(divider,'Main divider');
    await page.mouse.move(box.x + box.width / 2, box.y + 150);
    await page.mouse.down();
    await page.mouse.move(80, box.y + 150, { steps: 12 });
    await page.mouse.up();
    await settle();
    if ((await page.locator('#jobs-pane').evaluate((el) => el.getBoundingClientRect().width)) < 279)
      throw Error('Jobs pane escaped minimum');
    box = await visibleBox(divider,'Main divider');
    await page.mouse.move(box.x + box.width / 2, box.y + 150);
    await page.mouse.down();
    await page.mouse.move(3000, box.y + 150, { steps: 12 });
    await page.mouse.up();
    await settle();
    if (
      (await page.locator('#workspace-pane').evaluate((el) => el.getBoundingClientRect().width)) <
      419
    )
      throw Error('Workspace escaped minimum');
    const savedWidth = await page
      .locator('#jobs-pane')
      .evaluate((el) => el.getBoundingClientRect().width);
    await page.reload({ waitUntil: 'networkidle' });
    await settle();
    if (
      Math.abs(
        (await page.locator('#jobs-pane').evaluate((el) => el.getBoundingClientRect().width)) -
          savedWidth,
      ) > 2
    )
      throw Error('Pane size was not retained on reload');
    await page.evaluate(() => setWorkspaceView('documents'));
    await divider.dblclick();
    await settle();
    await audit('main-divider-drag-bounds-and-reset');
    box = await visibleBox(divider,'Main divider');
    await page.mouse.move(box.x + 5, box.y + 100);
    await page.mouse.down();
    await page.setViewportSize({ width: 600, height: 800 });
    await settle();
    if (await page.evaluate(() => document.documentElement.hasAttribute('data-pane-drag')))
      throw Error('Viewport change left a stale drag state');
    await page.mouse.up();
    await page.setViewportSize({ width: 1440, height: 900 });
    await settle();
    await audit('viewport-change-during-drag');
    const libraryDivider = page.locator('.document-layout>.resize-divider');
    if (await libraryDivider.isVisible()) {
      const b = await visibleBox(libraryDivider.locator('span'),'Library resize grip'),
        before = await page
          .locator('.document-library')
          .evaluate((el) => el.getBoundingClientRect().width);
      await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
      await page.mouse.down();
      await page.mouse.move(b.x + b.width / 2 + 80, b.y + b.height / 2, { steps: 10 });
      await page.mouse.up();
      await settle();
      if (
        Math.abs(
          (await page
            .locator('.document-library')
            .evaluate((el) => el.getBoundingClientRect().width)) - before,
        ) < 30
      )
        throw Error('Library divider did not resize');
    }
    await page.evaluate(
      (doc) =>
        showDocument({
          ...doc,
          text: 'Fictional Candidate\nEngineering experience\nProject coordination\nCommunication and documentation',
          coverage: { pages: 1, pages_without_selectable_text: [] },
        }),
      pdf,
    );
    await page.evaluate((doc) => previewPdf(doc), pdf);
    await settle();
    const reviewDivider = page.locator('.document-review-panes>.resize-divider');
    if (!(await reviewDivider.isVisible())) throw Error('Paired review divider missing');
    let r = await visibleBox(reviewDivider.locator('span'),'PDF/text resize grip');
    const beforeReview = await page
      .locator('#pdf-preview')
      .evaluate((el) => el.getBoundingClientRect().width);
    await page.mouse.move(r.x + r.width / 2, r.y + r.height / 2);
    await page.mouse.down();
    await page.mouse.move(r.x + r.width / 2 + 80, r.y + r.height / 2, { steps: 10 });
    await page.mouse.up();
    await settle();
    if (
      Math.abs(
        (await page.locator('#pdf-preview').evaluate((el) => el.getBoundingClientRect().width)) -
          beforeReview,
      ) < 30
    )
      throw Error('PDF/text divider did not resize');
    const picture = page.locator('.pdf-page-frame');
    await picture.scrollIntoViewIfNeeded();
    r = await visibleBox(picture,'PDF page frame');
    await page.mouse.move(r.x + r.width - 4, r.y + r.height - 4);
    await page.mouse.down();
    await page.mouse.move(r.x + r.width - 4, r.y + r.height - 94, { steps: 10 });
    await page.mouse.up();
    await settle();
    if ((await visibleBox(picture,'PDF page frame')).height > r.height - 40)
      throw Error('PDF page frame did not resize');
    const extracted = page.locator(
      '#document-preview textarea[aria-label="Extracted document text"]',
    );
    await extracted.scrollIntoViewIfNeeded();
    r = await visibleBox(extracted,'Extracted text area');
    await page.mouse.move(r.x + r.width - 4, r.y + r.height - 4);
    await page.mouse.down();
    await page.mouse.move(r.x + r.width - 4, r.y + r.height + 76, { steps: 10 });
    await page.mouse.up();
    await settle();
    if ((await visibleBox(extracted,'Extracted text area')).height < r.height + 30)
      throw Error('Extracted text area did not resize');
    await page.screenshot({ path: path.join(scratch, 'documents-desktop.png') });
    await audit('paired-pdf-text-review');
    await page.locator('#open-license').click();
    const dialog = page.locator('#license-dialog');
    let d = await visibleBox(dialog,'License dialog');
    await page.mouse.move(d.x + d.width - 4, d.y + d.height - 4);
    await page.mouse.down();
    await page.mouse.move(d.x + d.width - 324, d.y + d.height - 154, { steps: 15 });
    await page.mouse.up();
    await settle();
    const resized = await visibleBox(dialog,'License dialog');
    if (resized.width > d.width - 100) throw Error('License dialog did not resize');
    await audit('resized-license-dialog');
    await page.screenshot({ path: path.join(scratch, 'resized-license.png') });
    await page.locator('#close-license').click();
    for (const [width, height, factor] of [
      [1280, 960, 4],
      [1440, 900, 2],
      [1024, 768, 1],
      [768, 1024, 1],
      [390, 844, 1],
      [2560, 1440, 1.25],
    ]) {
      await page.setViewportSize({ width, height });
      await zoom(factor);
      await audit('paired-review-' + width + '@' + factor);
    }
    await page.setViewportSize({ width: 1440, height: 900 });
    await zoom(1);
    await page.evaluate(() => setWorkspaceView('settings'));
    await settle();
    if (
      (await page
        .locator('.settings-nav a[href="#settings-interface"]')
        .getAttribute('aria-current')) !== 'location'
    )
      throw Error('Default settings location marker is incorrect');
    for (const hash of [
      '#settings-interface',
      '#settings-storage',
      '#settings-email',
      '#local-tools',
    ]) {
      const link = page.locator('.settings-nav a[href="' + hash + '"]');
      await link.click();
      await settle();
      if ((await link.getAttribute('aria-current')) !== 'location')
        throw Error('Settings navigation did not mark its destination');
      const visibleHeading = await page.evaluate((hash) => {
        const target = document.querySelector(hash),
          tabs = document
            .querySelector('nav[aria-label="Workspace sections"]')
            .getBoundingClientRect();
        return target.getBoundingClientRect().top >= tabs.bottom - 1;
      }, hash);
      if (!visibleHeading) throw Error('Settings destination is hidden behind navigation: ' + hash);
      await audit('settings-navigation-' + hash.slice(1), hash + (hash === '#local-tools' ? '>summary' : '>h2'));
    }
    await page.locator('.settings-nav a[href="#settings-interface"]').click();
    await page.locator('#display-density').selectOption('compact');
    await page.locator('#display-motion').selectOption('reduced');
    await settle();
    const appearanceOptions = await page.evaluate(() => ({
      density: document.documentElement.dataset.density,
      motion: document.documentElement.dataset.motion,
    }));
    if (appearanceOptions.density !== 'compact' || appearanceOptions.motion !== 'reduced')
      throw Error('Appearance setting did not apply');
    const resetButton = page.locator('#reset-panel-sizes');
    await resetButton.hover();
    await page.mouse.down();
    const reducedStyle = await resetButton.evaluate((el) => ({
      transition: getComputedStyle(el).transitionDuration,
      transform: getComputedStyle(el).transform,
    }));
    await page.mouse.up();
    if (reducedStyle.transition !== '0s' || reducedStyle.transform !== 'none')
      throw Error('Reduced motion did not disable transitions and pressed movement');
    await page.locator('#display-motion').selectOption('system');
    await page.emulateMedia({ reducedMotion: 'reduce' });
    if ((await resetButton.evaluate((el) => getComputedStyle(el).transitionDuration)) !== '0s')
      throw Error('System reduced-motion preference was ignored');
    await page.emulateMedia({ reducedMotion: 'no-preference' });
    if ((await resetButton.evaluate((el) => getComputedStyle(el).transitionDuration)) === '0s')
      throw Error('Standard interaction feedback did not resume');
    await page.locator('#display-motion').selectOption('reduced');
    await audit('appearance-motion-preferences', '#settings-interface>h2');
    await resetButton.click();
    if ((await page.locator('.pdf-page-frame').evaluate((el) => el.style.height)) !== '')
      throw Error('Reset did not clear page height');
    await page.screenshot({ path: path.join(scratch, 'settings-desktop.png') });
    await page.setViewportSize({ width: 390, height: 844 });
    await page.evaluate(() => focusWorkspaceRegion('workspace-pane'));
    await settle();
    await page.screenshot({ path: path.join(scratch, 'settings-mobile.png') });
    if (
      (await page
        .locator('.display-options')
        .evaluate((el) => getComputedStyle(el).gridTemplateColumns.split(' ').length)) !== 1
    )
      throw Error('Narrow appearance controls did not stack');
    await page.evaluate(() => {
      window.__layoutOriginalSetItem = Storage.prototype.setItem;
      Storage.prototype.setItem = function (key, value) {
        if (String(key).startsWith('smart-jobseeker-'))
          throw new DOMException('Fictional blocked preference storage', 'SecurityError');
        return window.__layoutOriginalSetItem.call(this, key, value);
      };
    });
    await page.locator('#display-density').selectOption('standard');
    if (
      !(await page.locator('#layout-preferences-status').innerText()).includes(
        'blocked saving preferences',
      )
    )
      throw Error('Blocked preference storage was not reported');
    if ((await page.evaluate(() => document.documentElement.dataset.density)) !== 'standard')
      throw Error('Blocked storage prevented the appearance change');
    await audit('blocked-preference-storage');
    await page.evaluate(() => {
      Storage.prototype.setItem = window.__layoutOriginalSetItem;
      delete window.__layoutOriginalSetItem;
    });
    await page.locator('#display-density').selectOption('compact');
    if (
      !(await page.locator('#layout-preferences-status').innerText()).includes('Saved for this tab')
    )
      throw Error('Preference status did not recover after storage became available');
    for(const [width,height,factor] of [[1440,900,1],[768,1024,1],[320,844,1],[1280,960,4]]){
      await page.setViewportSize({width,height});
      await zoom(factor);
      const expected=await page.evaluate(job=>{
        const name='Avery'+ 'x'.repeat(175);
        const text='Dear hiring team,\n\nI built fictional measurement tools.\n\nKind regards,\n'+name;
        showLetter(job,{kind:'cover_letter',notice:'Fictional signature layout check.',paragraphs:[{text:'I built fictional measurement tools.',facts:[1]}],facts:['Built fictional measurement tools.'],framing:{opening:'Dear hiring team,',closing:'Kind regards,'},signature:{status:'available',text:name},text},'fictional-context');
        return text;
      },jobs[0].job);
      const workspaceButton=page.getByRole('button',{name:'Workspace',exact:true});
      if(await workspaceButton.isVisible())await workspaceButton.click();
      await page.locator('#packet p').filter({hasText:/^Averyx+$/}).scrollIntoViewIfNeeded();
      if(await page.locator('textarea[aria-label="Editable Cover letter draft"]').inputValue()!==expected)throw Error('Canonical signature text was lost in the editor');
      await audit('profile-signature-'+width+'@'+factor);
      if(width===1440||width===320)await page.screenshot({path:path.join(scratch,'signature-'+width+'.png')});
    }
    for(const status of ['missing','invalid']){
      await page.setViewportSize({width:320,height:844});
      await zoom(1);
      await page.evaluate(({job,status})=>showLetter(job,{kind:'cover_letter',notice:'Fictional missing-name check.',paragraphs:[{text:'I built fictional measurement tools.',facts:[1]}],facts:['Built fictional measurement tools.'],signature:{status}},'fictional-context'),{job:jobs[0].job,status});
      const workspaceButton=page.getByRole('button',{name:'Workspace',exact:true});
      if(await workspaceButton.isVisible())await workspaceButton.click();
      if(!await page.locator('#packet .review-label').filter({hasText:status==='invalid'?'profile name could not be used':'Add your name in Profile'}).isVisible())throw Error('Missing or invalid signature was not disclosed');
      await audit('profile-signature-'+status);
    }
    build = {
      executableSha256: crypto
        .createHash('sha256')
        .update(fs.readFileSync(executable))
        .digest('hex'),
      browser: await cdp.send('Browser.getVersion'),
      playwright: require(
        path.join(repo, 'target/layout-qa/node_modules/playwright-core/package.json'),
      ).version,
      compiledAssets: !options['source-assets'],
      commit: process.env.GITHUB_SHA || null,
      freeMemoryAfter: os.freemem(),
    };
    completed=true;
    if (errors.length || failures.length) process.exitCode = 1;
  } catch(error){
    fatal=error;process.exitCode=1;
    if(page)try{
      fs.writeFileSync(path.join(scratch,'failure-geometry.json'),JSON.stringify(await bounded(page.evaluate(()=>({viewport:{width:innerWidth,height:innerHeight},elements:[...document.querySelectorAll('.resize-divider,.pdf-page-frame,#license-dialog')].map(el=>({label:el.getAttribute('aria-label'),rect:el.getBoundingClientRect().toJSON()}))})),'Failure geometry',3000),null,2));
      await bounded(page.screenshot({path:path.join(scratch,'failure.png')}),'Failure screenshot',3000);
    }catch(captureError){errors.push('Failure capture: '+captureError.message);}
  }
  finally {
    const cleanupErrors=[];
    for(const [resource,label]of [[context,'Browser context'],[browser,'Browser connection']]){
      if(resource)try{await bounded(resource.close(),label+' shutdown',10000);}catch(error){cleanupErrors.push(label+': '+error.message);}
    }
    try{
      const exited=new Promise((resolve,reject)=>{
        if(child.exitCode!==null||child.signalCode!==null)return resolve();
        child.once('exit',resolve);child.once('error',reject);
      });
      if(child.exitCode===null&&child.signalCode===null)child.kill();
      await bounded(exited,'Local app shutdown',5000);
    }catch(error){cleanupErrors.push(error.message);}
    if(cleanupErrors.length)process.exitCode=1;
    const redact=text=>String(text).replace(/[a-f0-9]{64}/gi,'[redacted]');
    fs.writeFileSync(path.join(scratch,'results.json'),JSON.stringify({completed,phase,build,cases,failures,scriptErrors:errors.map(redact),error:fatal?redact(fatal.stack||fatal.message):null,cleanup:{errors:cleanupErrors.map(redact),appExited:child.exitCode!==null||child.signalCode!==null}},null,2));
    console.log(JSON.stringify({scratch,completed,compiledAssets:!options['source-assets'],cases:cases.length,failures:failures.length,scriptErrors:errors.length,phase,cleanupErrors:cleanupErrors.length,examples:failures.slice(0,5)}));
    if(fatal)console.error(redact(fatal.stack||fatal.message));
  }
})().catch((error) => {
  console.error(error.message.replace(/[a-f0-9]{64}/g, '[redacted]'));
  process.exitCode = 1;
});
