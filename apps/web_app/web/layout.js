/* Shared pane sizing. Preferences contain proportions only and last for this tab. */
(function (root) {
  'use strict';
  const defaults = Object.freeze({ jobs: 0.32, library: 0.44, review: 0.5 });
  const storageKey = 'smart-jobseeker-layout-v1';

  function readSizes(storage) {
    const result = { ...defaults };
    try {
      const raw = storage.getItem(storageKey);
      if (!raw || raw.length > 256) return result;
      const saved = JSON.parse(raw);
      for (const key of Object.keys(defaults)) {
        if (
          typeof saved?.[key] === 'number' &&
          Number.isFinite(saved[key]) &&
          saved[key] >= 0.15 &&
          saved[key] <= 0.85
        )
          result[key] = saved[key];
      }
    } catch (_) {
      /* A denied browser store must not disable resizing. */
    }
    return result;
  }

  function splitSize(width, ratio, minimumFirst, minimumSecond) {
    const available = width - 10;
    if (
      !Number.isFinite(width) ||
      !Number.isFinite(ratio) ||
      available < minimumFirst + minimumSecond
    )
      return null;
    const lower = Math.max(minimumFirst, Math.ceil(available * 0.15));
    const upper = Math.min(available - minimumSecond, Math.floor(available * 0.85));
    return lower > upper ? null : Math.min(upper, Math.max(lower, Math.round(available * ratio)));
  }

  function readDisplay(storage) {
    const result = { density: 'standard', motion: 'system' };
    try {
      const raw = storage.getItem('smart-jobseeker-display-v1');
      if (!raw || raw.length > 128) return result;
      const saved = JSON.parse(raw);
      if (saved?.density === 'compact') result.density = 'compact';
      if (saved?.motion === 'reduced') result.motion = 'reduced';
    } catch (_) {}
    return result;
  }

  if (typeof module !== 'undefined' && module.exports)
    module.exports = { readSizes, splitSize, readDisplay };
  if (!root.document) return;
  const document = root.document;
  const preferenceStatus = document.querySelector('#layout-preferences-status');
  const savedNotice = preferenceStatus?.textContent || '';
  const storageUnavailable = () => {
    if (preferenceStatus)
      preferenceStatus.textContent =
        'This browser blocked saving preferences. Changes apply until this page is refreshed.';
  };
  let storage;
  try {
    storage = root.sessionStorage;
  } catch (_) {
    storage = null;
    storageUnavailable();
  }
  const reader = {
    getItem(key) {
      try {
        return storage?.getItem(key);
      } catch (error) {
        storageUnavailable();
        throw error;
      }
    },
  };
  const writePreference = (key, value) => {
    try {
      if (!storage) throw Error('Browser storage unavailable');
      storage.setItem(key, value);
      if (preferenceStatus) preferenceStatus.textContent = savedNotice;
    } catch (_) {
      storageUnavailable();
    }
  };
  const sizes = readSizes(reader);
  const splits = [];
  let frame = null;
  const schedule = () => {
    if (frame !== null) return;
    frame = root.requestAnimationFrame(() => {
      frame = null;
      for (const split of splits) split.refresh();
    });
  };
  const save = () => {
    writePreference(storageKey, JSON.stringify(sizes));
  };

  function register(container, first, second, key, minimumFirst, minimumSecond, enabled, label) {
    if (!container || !first || !second) return;
    const divider = document.createElement('div');
    divider.className = 'resize-divider';
    divider.setAttribute('role', 'separator');
    divider.setAttribute('aria-orientation', 'vertical');
    divider.setAttribute('aria-label', label);
    divider.title = 'Drag to resize. Double-click to restore the default size.';
    const grip = document.createElement('span');
    grip.textContent = '⋮';
    grip.setAttribute('aria-hidden', 'true');
    divider.append(grip);
    first.after(divider);
    first.classList.add('split-first');
    second.classList.add('split-second');
    let drag = null,
      previous = '';
    const cancel = () => {
      if (!drag) return;
      const id = drag.pointer;
      drag = null;
      document.documentElement.removeAttribute('data-pane-drag');
      divider.classList.remove('dragging');
      if (divider.hasPointerCapture(id)) divider.releasePointerCapture(id);
    };
    const refresh = () => {
      const width = container.getBoundingClientRect().width;
      const pixels = enabled(width)
        ? splitSize(width, sizes[key], minimumFirst, minimumSecond)
        : null;
      if (pixels === null) cancel();
      const signature = pixels === null ? 'off' : `${pixels}:${width}`;
      if (signature === previous) return;
      previous = signature;
      divider.hidden = pixels === null;
      container.classList.toggle('resizable-split', pixels !== null);
      if (pixels === null) container.style.removeProperty('--split-columns');
      else container.style.setProperty('--split-columns', `${pixels}px 10px minmax(0, 1fr)`);
    };
    divider.addEventListener('pointerdown', (event) => {
      if (event.button !== 0 || drag || divider.hidden) return;
      event.preventDefault();
      drag = { pointer: event.pointerId, original: sizes[key] };
      divider.setPointerCapture(event.pointerId);
      divider.classList.add('dragging');
      document.documentElement.dataset.paneDrag = 'columns';
    });
    divider.addEventListener('pointermove', (event) => {
      if (!drag || drag.pointer !== event.pointerId) return;
      const box = container.getBoundingClientRect();
      const pixels = splitSize(
        box.width,
        (event.clientX - box.left) / (box.width - 10),
        minimumFirst,
        minimumSecond,
      );
      if (pixels === null) return;
      sizes[key] = pixels / (box.width - 10);
      schedule();
    });
    divider.addEventListener('pointerup', (event) => {
      if (!drag || drag.pointer !== event.pointerId) return;
      cancel();
      save();
      schedule();
    });
    divider.addEventListener('pointercancel', () => {
      if (drag) sizes[key] = drag.original;
      cancel();
      schedule();
    });
    divider.addEventListener('lostpointercapture', () => {
      if (drag) {
        cancel();
        save();
        schedule();
      }
    });
    divider.addEventListener('dblclick', () => {
      cancel();
      sizes[key] = defaults[key];
      save();
      schedule();
    });
    splits.push({ refresh, cancel });
    new root.ResizeObserver(schedule).observe(container);
    refresh();
  }

  const pdf = document.querySelector('#pdf-preview');
  const text = document.querySelector('#document-preview');
  const pairedReview = () => Boolean(pdf?.childElementCount && text?.childElementCount);
  register(
    document.querySelector('main'),
    document.querySelector('#jobs-pane'),
    document.querySelector('#workspace-pane'),
    'jobs',
    280,
    420,
    () => root.innerWidth > 850,
    'Resize jobs and workspace',
  );
  register(
    document.querySelector('.document-layout'),
    document.querySelector('.document-library'),
    document.querySelector('.document-preview-pane'),
    'library',
    300,
    320,
    (width) => width >= 740 && !pairedReview(),
    'Resize document library and preview',
  );
  register(
    document.querySelector('.document-review-panes'),
    pdf,
    text,
    'review',
    260,
    280,
    (width) => width >= 640 && pairedReview(),
    'Resize PDF and extracted text',
  );
  new root.MutationObserver(schedule).observe(document.querySelector('#documents-section'), {
    childList: true,
    subtree: true,
  });
  root.addEventListener('resize', schedule);
  root.addEventListener('blur', () => {
    for (const split of splits) split.cancel();
    save();
  });

  const display = readDisplay(reader);
  const density = document.querySelector('#display-density');
  const motion = document.querySelector('#display-motion');
  const applyDisplay = () => {
    document.documentElement.dataset.density = display.density;
    document.documentElement.dataset.motion = display.motion;
    if (density) density.value = display.density;
    if (motion) motion.value = display.motion;
    schedule();
  };
  for (const [control, key, values] of [
    [density, 'density', ['standard', 'compact']],
    [motion, 'motion', ['system', 'reduced']],
  ]) {
    control?.addEventListener('change', () => {
      if (!values.includes(control.value)) return;
      display[key] = control.value;
      applyDisplay();
      writePreference('smart-jobseeker-display-v1', JSON.stringify(display));
    });
  }
  document.querySelector('#reset-panel-sizes')?.addEventListener('click', () => {
    Object.assign(sizes, defaults);
    for (const split of splits) split.cancel();
    for (const element of document.querySelectorAll('textarea,.pdf-page-frame,#license-dialog')) {
      element.style.removeProperty('height');
      element.style.removeProperty('width');
    }
    save();
    schedule();
  });
  applyDisplay();

  const settingsMarker = () => {
    const links = [...document.querySelectorAll('.settings-nav a')];
    const selected = links.some((link) => link.hash === root.location.hash)
      ? root.location.hash
      : '#settings-interface';
    for (const link of links) {
      if (link.hash === selected) link.setAttribute('aria-current', 'location');
      else link.removeAttribute('aria-current');
    }
  };
  settingsMarker();
  root.addEventListener('hashchange', settingsMarker);
  new root.MutationObserver(settingsMarker).observe(document.querySelector('aside'), {
    attributes: true,
    attributeFilter: ['data-view'],
  });
  document.querySelector('#open-job-search')?.addEventListener('click', () => {
    const query = document.querySelector('#web-search input[name="query"]');
    if (!query) return;
    for (let parent = query.parentElement; parent; parent = parent.parentElement) {
      if (parent.tagName === 'DETAILS') parent.open = true;
    }
    query.scrollIntoView({ behavior: 'instant', block: 'center' });
    query.focus({ preventScroll: true });
  });
})(typeof window !== 'undefined' ? window : globalThis);
