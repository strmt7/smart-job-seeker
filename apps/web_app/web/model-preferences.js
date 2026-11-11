(() => {
  'use strict';
  let client, el, root, selected, folder, refreshButton, clearButton;
  let revision = null, choice = null, busy = false, dirty = false;
  function valid(value) {
    return value && Number.isSafeInteger(value.revision) && value.revision >= 0
      && (value.choice === null || (typeof value.choice?.name === 'string'
        && /^[0-9a-f]{64}$/i.test(value.choice.digest)
        && Number.isSafeInteger(value.choice.size_bytes)
        && (value.choice.directory === null || typeof value.choice.directory === 'string')));
  }
  function apply(value) {
    if (!valid(value)) throw Error('Model settings could not be verified.');
    revision = value.revision;
    choice = value.choice;
    if (!dirty) folder.value = choice?.directory || '';
    selected.textContent = choice ? `Rehearsal model: ${choice.name}. Bound to this installed model version.` : 'No rehearsal model selected. A qualified loaded model can be used.';
  }
  function controls() {
    if (!root) return;
    refreshButton.disabled = busy;
    clearButton.disabled = busy || revision === null || choice === null;
    folder.disabled = busy;
    for (const button of document.querySelectorAll('[data-rehearsal-model]')) {
      button.disabled = busy || button.getAttribute('data-supported') !== 'true';
    }
  }
  async function refresh() {
    if (busy) return;
    busy = true; controls();
    try { apply(await client('/api/model-preferences')); }
    catch (error) { selected.textContent = error.message; }
    finally { busy = false; controls(); }
  }
  async function save(model) {
    if (busy || !client) return;
    busy = true; controls();
    try {
      if (revision === null) apply(await client('/api/model-preferences'));
      const result = await client('/api/model-preferences', {revision, model, directory: model ? folder.value.trim() || null : null});
      dirty = false; apply(result);
    } catch (error) { selected.textContent = `${error.message} Refresh the selection before retrying.`; }
    finally { busy = false; controls(); }
  }
  function mount(container, dependencies) {
    if (root) return;
    client = dependencies.api; el = dependencies.element; root = container;
    const heading = el('h4', 'Rehearsal model');
    const hint = el('p', 'Choose an installed local vision model below. The selection is saved for this workspace and checked by version before use.', 'footnote');
    selected = el('p', 'Reading model selection...', 'footnote'); selected.setAttribute('role', 'status');
    const label = el('label', 'Model storage folder (optional)');
    folder = el('input'); folder.type = 'text'; folder.maxLength = 1024;
    folder.placeholder = 'Detect existing Ollama storage';
    folder.addEventListener('input', () => { dirty = true; }); label.append(folder);
    const pathHint = el('p', 'For a custom installation, choose the local folder containing Ollama manifests and blobs.', 'footnote');
    const actions = el('div', '', 'source-copy-actions');
    refreshButton = el('button', 'Refresh selection', 'button-secondary'); refreshButton.type = 'button';
    clearButton = el('button', 'Clear selection', 'button-secondary'); clearButton.type = 'button';
    refreshButton.addEventListener('click', refresh); clearButton.addEventListener('click', () => save(null));
    actions.append(refreshButton, clearButton); root.append(heading, hint, selected, label, pathHint, actions);
    void refresh();
  }
  function forModel(model, supported, element) {
    const button = element('button', 'Use for rehearsal', 'button-secondary'); button.type = 'button';
    button.setAttribute('data-rehearsal-model', 'true'); button.setAttribute('data-supported', String(supported));
    button.disabled = busy || !supported;
    button.title = 'Save this model version for visual rehearsal';
    button.addEventListener('click', () => save(model.name));
    return button;
  }
  window.WaypointModelPreferences = {mount, forModel};
})();
