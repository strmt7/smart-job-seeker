(rowIndex => {
  const rows = [];
  let selected = null;
  const result = { rows, incomplete: false,
    viewport: [scrollX, scrollY, innerWidth, innerHeight, devicePixelRatio] };
  const headings = 'legend, h2, h3, h4, h5, h6, [role="heading"]';
  const groups = 'section, fieldset, [role="region"], [role="group"]';
  const term = 'dt, [role="term"]';
  const definition = 'dd, [role="definition"]';
  const rowHeader = 'th:not([scope="col" i], [scope="colgroup" i]), [role="rowheader"]';
  const cell = 'td, [role="cell"]';
  const visible = node => node.checkVisibility({
    opacityProperty: true, visibilityProperty: true, contentVisibilityAuto: true,
  }) && !node.closest('[aria-hidden="true" i], [inert]');
  const renderedLabel = node => node.innerText.trim().replace(/\s+/gu, ' ');
  const encoder = new TextEncoder();
  let bytes = 0;

  function textBounds(node) {
    const range = document.createRange();
    range.selectNodeContents(node);
    const rect = range.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return null;
    const inViewport = rect.left >= 0 && rect.top >= 0
      && rect.right <= innerWidth && rect.bottom <= innerHeight;
    const visible = inViewport && Array.from(range.getClientRects()).every(part => {
      if (part.width <= 0 || part.height <= 0) return true;
      const target = document.elementFromPoint(part.x + part.width / 2, part.y + part.height / 2);
      return target === node || node.contains(target);
    });
    // Page clips cannot reveal a section hidden inside a different scrollport
    // or reliably place a viewport-anchored element at document coordinates.
    let pageClip = !inViewport || visible;
    for (let owner = node; pageClip && owner; owner = owner.parentElement) {
      const style = getComputedStyle(owner);
      if (['fixed', 'sticky'].includes(style.position) || style.clipPath !== 'none'
          || style.maskImage !== 'none' || style.contain.split(' ').some(value =>
            ['paint', 'strict', 'content'].includes(value))) {
        pageClip = false;
      } else if (owner !== document.body && owner !== document.documentElement
          && (style.overflowX !== 'visible' || style.overflowY !== 'visible')) {
        const box = owner.getBoundingClientRect();
        const left = box.left + owner.clientLeft;
        const top = box.top + owner.clientTop;
        pageClip = style.transform === 'none'
          && (style.overflowX === 'visible' || (rect.left >= left && rect.right <= left + owner.clientWidth))
          && (style.overflowY === 'visible' || (rect.top >= top && rect.bottom <= top + owner.clientHeight));
      }
    }
    return { document: [rect.x + scrollX, rect.y + scrollY, rect.width, rect.height],
      in_image: visible, page_clip: pageClip };
  }

  function groupHeadings(label) {
    const sectionNodes = [];
    for (let parent = label.parentElement; parent; parent = parent.parentElement) {
      if (!parent.matches(groups)) continue;
      const reference = parent.getAttribute('aria-labelledby');
      const titles = reference
        ? reference.trim().split(/\s+/u).map(id => document.getElementById(id))
        : Array.from(parent.children).filter(node => node.matches(headings) && visible(node));
      if (titles.length > 1 || titles.some(node => !node || !parent.contains(node) || !visible(node))) {
        return null;
      }
      if (titles.length === 1) sectionNodes.push(titles[0]);
    }
    return sectionNodes.reverse();
  }

  function record(label, value) {
    const sectionNodes = groupHeadings(label);
    if (!sectionNodes || !value || !visible(value)) {
      result.incomplete = true;
      return;
    }
    const key = { sections: sectionNodes.map(renderedLabel), label: renderedLabel(label) };
    const displayed = value.innerText;
    if (![...key.sections, key.label].every(text => text && encoder.encode(text).length <= 1000)
        || encoder.encode(displayed).length > 1000) {
      result.incomplete = true;
      return;
    }
    const nodes = [...sectionNodes, label, value];
    const rects = nodes.map(textBounds);
    const entry = { answer: { key, value: displayed },
      bounds: rects.some(rect => rect === null) ? null : rects };
    bytes += encoder.encode(JSON.stringify(entry)).length;
    if (bytes > 96 * 1024) {
      result.incomplete = true;
      return;
    }
    if (rows.length === rowIndex) selected = nodes;
    rows.push(entry);
  }

  const tree = document.createTreeWalker(document.body, NodeFilter.SHOW_ELEMENT);
  let scanned = 0;
  for (let node = tree.nextNode(); node; node = tree.nextNode()) {
    if (++scanned > 20_000 || bytes > 96 * 1024) {
      result.incomplete = true;
      break;
    }
    if (node.matches(term) && node.closest('dl, [role="list"], [role="group"]') && visible(node)) {
      const value = node.nextElementSibling;
      const singlePair = value?.matches(definition)
        && !node.previousElementSibling?.matches(term)
        && !value.nextElementSibling?.matches(definition);
      record(node, singlePair ? value : null);
    } else if (node.matches('tr, [role="row"]') && node.closest('table, [role="table"]') && visible(node)) {
      const cells = Array.from(node.children).filter(child => child.matches(cell) && visible(child));
      if (!cells.length) continue;
      const labels = Array.from(node.children).filter(child => child.matches(rowHeader) && visible(child));
      if (labels.length === 1 && cells.length === 1 && labels[0].nextElementSibling === cells[0]) {
        record(labels[0], cells[0]);
      } else {
        result.incomplete = true;
      }
    }
  }
  return rowIndex === null ? result : { result, selected };
})
