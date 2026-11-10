// Explicit developer qualification: private workspace, headless UI, public reference page only.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawn } = require('node:child_process');
const { chromium } = require('../../../target/layout-qa/node_modules/playwright-core');
const folder = path.resolve(process.env.WAYPOINT_SOURCE_QA_FOLDER || '');
const browserPath = process.env.WAYPOINT_SOURCE_QA_BROWSER;
const executable = path.resolve(__dirname, '../../../target/debug/waypoint_web.exe');
if (!process.env.WAYPOINT_SOURCE_QA_FOLDER || !browserPath) throw Error('Use tools/qualify_source_copy.py');
let child, context, browser, token, appErrors = '', origin;
const evidence = { complete: false, scope: 'Anonymous public reference-page copy; no employer form or candidate interaction', cases: [], screenshots: [] };
function assert(condition, message) { if (!condition) throw Error(message); }
async function bounded(operation, milliseconds = 15000) {
  let timer;
  try { return await Promise.race([operation, new Promise((_, reject) => { timer = setTimeout(() => reject(Error('Qualification operation timed out')), milliseconds); })]); }
  finally { clearTimeout(timer); }
}
async function api(route, body) {
  const response = await bounded(fetch(origin + route, { headers: { 'X-Workspace-Token': token, 'Content-Type': 'application/json' }, ...(body ? { method: 'POST', body: JSON.stringify(body) } : {}) }));
  const value = await response.json();
  assert(response.ok, value.error || 'API request failed');
  return value;
}
async function waitFor(check, seconds = 120) {
  const end = Date.now() + seconds * 1000;
  while (Date.now() < end) { const value = await check(); if (value) return value; await new Promise(resolve => setTimeout(resolve, 200)); }
  throw Error('Expected state was not observed before timeout');
}
async function expand(page) {
  const detail = page.locator('#jobs .job-more').first();
  await detail.waitFor();
  await detail.evaluate(node => { node.open = true; });
  return detail.locator('.source-copy');
}
(async () => {
  try {
    child = spawn(executable, [], { windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, APPDATA: path.join(folder, 'appdata'), WAYPOINT_WORKSPACE_DIR: path.join(folder, 'workspace'), WAYPOINT_NO_BROWSER: '1', WAYPOINT_NO_TRAY: '1' } });
    child.stderr.on('data', bytes => { if (appErrors.length < 32768) appErrors += bytes.toString(); });
    const address = await bounded(new Promise((resolve, reject) => {
      let output = '';
      child.stdout.on('data', bytes => {
        output += bytes;
        if (output.length > 65536) return reject(Error('Startup output exceeded its limit'));
        const match = output.match(/http:\/\/127\.0\.0\.1:\d+\/#([a-f0-9]{64})/);
        if (match) { token = match[1]; resolve(match[0]); }
      });
      child.once('error', reject); child.once('exit', () => reject(Error('App exited before startup')));
    }), 30000);
    origin = new URL(address).origin;
    const added = await api('/api/jobs/add', { url: 'https://www.rfc-editor.org/rfc/rfc9309.html', title: 'Fictional source-copy qualification', employer: 'Reference fixture', description: 'Fictional local record. Public technical reference used to qualify anonymous resource capture.' });
    const extensionPath = path.join(folder, 'extension'), profile = path.join(folder, 'browser');
    fs.mkdirSync(extensionPath);
    fs.writeFileSync(path.join(extensionPath, 'manifest.json'), JSON.stringify({ manifest_version: 3, name: 'Source copy layout qualification', version: '1.0', permissions: ['tabs'] }));
    fs.writeFileSync(path.join(extensionPath, 'control.html'), '<!doctype html><title>Layout controller</title>');
    context = await chromium.launchPersistentContext(profile, { executablePath: browserPath, headless: true, viewport: { width: 1280, height: 900 }, args: ['--enable-unsafe-extension-debugging', '--remote-debugging-port=0'], ignoreDefaultArgs: ['--disable-extensions'] });
    await context.route('**/*', route => new URL(route.request().url()).origin === origin || route.request().url().startsWith('chrome-extension:') ? route.continue() : route.abort());
    const port = fs.readFileSync(path.join(profile, 'DevToolsActivePort'), 'utf8').split('\n')[0];
    browser = await chromium.connectOverCDP('http://127.0.0.1:' + port);
    const session = await browser.newBrowserCDPSession();
    const extension = await session.send('Extensions.loadUnpacked', { path: extensionPath });
    const page = await context.newPage(), errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(address, { waitUntil: 'networkidle' });
    const controller = await context.newPage();
    await controller.goto('chrome-extension://' + extension.id + '/control.html');
    await page.bringToFront();
    let panel = await expand(page);
    await panel.getByRole('button', { name: 'Save page copy', exact: true }).click();
    const running = await waitFor(async () => { const pending = await api('/api/job-source/pending', {}); return pending.active && pending; }, 30);
    assert(running.job_id === added.id, 'Running capture belongs to another job');
    await panel.getByRole('button', { name: 'Stop current copy', exact: true }).click();
    const cancelled = await waitFor(async () => { const result = await api('/api/job-source/status', { task_id: running.task_id }); return result.status !== 'running' && result; }, 30);
    assert(cancelled.status === 'cancelled' && cancelled.record_saved === true, 'Capture cancellation was not confirmed and preserved');
    await waitFor(async () => (await panel.locator('[role=status]').textContent()).startsWith('Stopped.'));
    evidence.cancel = cancelled;
    evidence.cases.push('Rendered start/stop confirmed owned worker shutdown and retained record');
    await panel.getByRole('button', { name: 'Save another copy', exact: true }).click();
    const second = await waitFor(async () => { const pending = await api('/api/job-source/pending', {}); return pending.active && pending; }, 30);
    assert(second.task_id !== running.task_id, 'Second copy reused its attempt identity');
    const result = await waitFor(async () => { const value = await api('/api/job-source/status', { task_id: second.task_id }); return value.status !== 'running' && value; }, 180);
    evidence.capture = result;
    assert(result.status === 'complete' && result.record_saved === true && result.result.archive_written === true, 'Public reference copy did not produce an archived record: ' + (result.error || result.status));
    assert(result.current_source === true && result.result.report.recorded_resources >= 1, 'Capture source/resources were not established');
    const outline=result.result.report.discovery?.form_outline;
    assert(outline && ['observed','partial'].includes(outline.status) && Array.isArray(outline.controls),'Public page controls were not observed');
    assert(result.result.report.discovery.form_outline_snapshot_sha256?.length===64,'Public controls lost their checkpoint binding');
    await waitFor(async () => (await panel.locator('[role=status]').textContent()).includes('Page copy saved'));
    evidence.cases.push('Rendered repeat copy used native public GET coordinator and render-only resource discovery');
    await page.reload({ waitUntil: 'networkidle' });
    panel = await expand(page);
    await waitFor(async () => (await panel.locator('[role=status]').textContent()).includes('Page copy saved'));
    const history = await api('/api/attempts/list', { id: added.id });
    assert(history.attempts.length === 2 && history.attempts.some(row => row.source_page_copy?.archive_written === true) && history.attempts.some(row => row.source_page_copy?.status === 'cancelled'), 'Attempt history lost a capture outcome');
    assert(history.attempts.some(row=>row.source_page_copy?.form_outline?.controls?.length===outline.controls.length),'History lost the public control inventory');
    const state = await api('/api/state');
    assert(state.jobs.find(row => row.job.id === added.id)?.job.state === 'discovered', 'Source capture changed application state');
    evidence.cases.push('Reload recovered terminal copy; history retained cancellation and archive; job remained discovered');
    const captureDetails = panel.locator('.source-copy-result');
    await captureDetails.evaluate(node => { node.open = true; });
    const outlinePanel=panel.locator('.source-form-outline');
    await outlinePanel.evaluate(node=>{node.open=true;});
    if(outline.controls.length>12){await outlinePanel.getByRole('button',{name:`Show all ${outline.controls.length} controls`,exact:true}).click();assert(await outlinePanel.locator('li').count()===outline.controls.length,'Control expansion lost rows');await outlinePanel.getByRole('button',{name:'Show fewer controls',exact:true}).click();}
    for (const width of [320, 700, 1280]) {
      await page.setViewportSize({ width, height: 1000 });
      for (const factor of [0.8, 1, 2, 4]) {
        if (width / factor < 319 || 1000 / factor < 239) continue;
        const actual = await controller.evaluate(async ({ origin, factor }) => {
          const [tab] = await chrome.tabs.query({ url: origin + '/*' });
          if (!tab) throw Error('App tab missing');
          await chrome.tabs.setZoomSettings(tab.id, { mode: 'automatic', scope: 'per-tab' });
          await chrome.tabs.setZoom(tab.id, factor); return chrome.tabs.getZoom(tab.id);
        }, { origin, factor });
        assert(Math.abs(actual - factor) < .01, 'Actual browser zoom did not apply');
        await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
        await panel.scrollIntoViewIfNeeded();
        const measure = await panel.evaluate(node => {
          const rect = node.getBoundingClientRect();
          return { width: innerWidth, document: document.documentElement.scrollWidth, panelWidth: rect.width, clientWidth: node.clientWidth, scrollWidth: node.scrollWidth,
            clippedControls: [...node.querySelectorAll('button,progress,summary')].filter(el => !el.hidden && el.getClientRects().length).filter(el => { const r = el.getBoundingClientRect(); return r.left < rect.left - 2 || r.right > rect.right + 2; }).length };
        });
        assert(measure.document <= measure.width + 2 && measure.scrollWidth <= measure.clientWidth + 2 && measure.clippedControls === 0, `Capture panel overflow at ${width}px / ${factor} zoom: ${JSON.stringify(measure)}`);
        evidence.cases.push({ viewport_width: width, zoom: actual, ...measure });
        if ((width === 320 && factor === 1) || (width === 1280 && factor === 1) || (width === 1280 && factor === 4)) {
          const name = `source-copy-${width}-${factor}.png`;
          await outlinePanel.evaluate(node => node.scrollIntoView({ block: 'start', behavior: 'instant' }));
          // Explicit surface capture avoids zoom/scroll clip-coordinate conversion.
          const screen = await context.newCDPSession(page);
          const shot = await screen.send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
          fs.writeFileSync(path.join(folder, name), Buffer.from(shot.data, 'base64'));
          await screen.detach();
          evidence.screenshots.push({ name, sha256: crypto.createHash('sha256').update(fs.readFileSync(path.join(folder, name))).digest('hex') });
        }
      }
    }
    assert(errors.length === 0, 'Rendered client errors: ' + errors.join('; '));
    evidence.rendered_errors = errors;
    evidence.complete = true;
  } catch (error) { evidence.error = error.message; process.exitCode = 1; }
  finally {
    if (context) await context.close().catch(() => {});
    if (browser) await browser.close().catch(() => {});
    if (child && child.exitCode === null) {
      const ended = new Promise(resolve => child.once('exit', resolve));
      child.kill();
      await bounded(ended, 5000).catch(() => { evidence.app_shutdown_unconfirmed = true; evidence.complete = false; process.exitCode = 1; });
    }
    if (appErrors) evidence.app_errors = token ? appErrors.split(token).join('[redacted]') : appErrors;
    fs.writeFileSync(path.join(folder, 'rendered.private.json'), JSON.stringify(evidence, null, 2));
    console.log(JSON.stringify({ complete: evidence.complete, cases: evidence.cases.length, error: evidence.error || null }));
  }
})();
