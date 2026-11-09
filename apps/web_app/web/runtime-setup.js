/* Private dependency setup. Shared rendering uses text nodes only. */
globalThis.SmartJobseekerToolSetup = {
  connect({form, info, api, storage, wait = ms => new Promise(resolve => setTimeout(resolve, ms))}) {
    const key = 'smart-jobseeker-tool-setup', button = form.querySelector('button');
    let following = false;
    async function follow(task) {
      if (following) return;
      following = true; button.disabled = true;
      try {
        for (let count = 0; count < 3000; count++) {
          const result = await api('/api/tools-setup/status', {task_id: task});
          if (result.status === 'running') {
            info.textContent = result.phase || 'Setting up private tools…';
            await wait(1500); continue;
          }
          storage.removeItem(key);
          if (result.status !== 'complete') throw new Error(result.error || 'Tool setup failed');
          const names = {pdf: 'PDF tools', browser: 'browser', office: 'document converter', ollama: 'Ollama'};
          const components = result.result?.components || [];
          const warnings = components.map(item => item.cleanup_warning).filter(Boolean);
          info.textContent = `Tools ready: ${components.map(item => names[item.tool || item.runtime?.tool] || 'local tool').join(', ')}.${warnings.length ? ` ${warnings.join(' ')}` : ''}`;
          return;
        }
        info.textContent = 'Setup may still be running. Check its status again before starting another.';
      } catch (error) {
        if (error.message === 'Tool setup not found') storage.removeItem(key);
        info.textContent = error.message;
      } finally { following = false; button.disabled = false; }
    }
    async function recover() {
      let task = storage.getItem(key);
      if (!task) {
        const result = await api('/api/tools-setup/pending', {});
        task = result.task_id;
        if (task) storage.setItem(key, task);
      }
      if (task) { await follow(task); return true; }
      return false;
    }
    async function start(event) {
      event?.preventDefault();
      if (following) return;
      button.disabled = true; info.textContent = 'Checking the tools selected for setup…';
      try {
        let task = storage.getItem(key);
        if (!task) {
          const result = await api('/api/tools-setup/start', {component: form.elements.component.value});
          task = result.task_id;
          if (!task) throw new Error('Setup did not return a task identity');
          storage.setItem(key, task);
        }
        await follow(task);
      } catch (error) {
        info.textContent = error.message;
        try { await recover(); } catch (_) { info.textContent = `${error.message}. Could not recover setup status; check again before retrying.`; }
      } finally { button.disabled = false; }
    }
    form.addEventListener('submit', start);
    return {start, recover};
  }
};
if (typeof document !== 'undefined') {
  const form = document.querySelector('#tools-setup-form');
  if (form) {
    const info = document.querySelector('#tools-setup-info');
    const setup = SmartJobseekerToolSetup.connect({form, info, api, storage: sessionStorage});
    void setup.recover().catch(() => { info.textContent = 'Could not check for an active setup. Check again before installing tools.'; });
  }
}
