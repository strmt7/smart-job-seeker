((inspect, row) => {
  const { selected } = inspect(row);
  const root = document.scrollingElement;
  if (!selected || !root) return null;

  const anchors = new Map();
  const scrollable = new Set(['auto', 'scroll', 'hidden']);
  const unscaled = style => style.transform === 'none' && style.perspective === 'none'
    && style.translate === 'none' && style.rotate === 'none' && style.scale === 'none'
    && Number(style.zoom) === 1;

  function ancestors(node) {
    const result = [];
    for (let owner = node; owner; owner = owner.parentElement) {
      if (result.length >= 64) return null;
      result.push(owner);
    }
    return result;
  }

  for (const node of selected) {
    const chain = ancestors(node);
    if (!chain) return null;
    for (const owner of chain) {
      if (owner === root || owner.scrollWidth > owner.clientWidth || owner.scrollHeight > owner.clientHeight) {
        anchors.set(owner, [owner.scrollLeft, owner.scrollTop]);
      }
    }
  }

  function parts(index) {
    const headingCount = selected.length - 2;
    return index < headingCount ? [selected[index]] : selected.slice(headingCount);
  }

  function rect(node) {
    const range = document.createRange();
    range.selectNodeContents(node);
    return range.getBoundingClientRect();
  }

  function clientEdges(owner) {
    if (owner === root) return [0, 0, root.clientWidth, root.clientHeight];
    const box = owner.getBoundingClientRect();
    const left = box.left + owner.clientLeft;
    const top = box.top + owner.clientTop;
    return [left, top, left + owner.clientWidth, top + owner.clientHeight];
  }

  function moveTo(nodes) {
    const chain = ancestors(nodes[0]);
    if (!chain || chain.some(owner => !unscaled(getComputedStyle(owner)))) return false;
    for (const owner of chain) {
      if (!anchors.has(owner) || !nodes.every(node => owner.contains(node))) continue;
      const style = getComputedStyle(owner);
      const [left, top, right, bottom] = clientEdges(owner);
      const boxes = nodes.map(rect);
      const x = Math.min(...boxes.map(box => box.left));
      const y = Math.min(...boxes.map(box => box.top));
      const endX = Math.max(...boxes.map(box => box.right));
      const endY = Math.max(...boxes.map(box => box.bottom));
      const dx = (owner === root || scrollable.has(style.overflowX)) && (x < left || endX > right)
        ? (x + endX - left - right) / 2 : 0;
      const dy = (owner === root || scrollable.has(style.overflowY)) && (y < top || endY > bottom)
        ? (y + endY - top - bottom) / 2 : 0;
      if (dx || dy) owner.scrollTo({ left: owner.scrollLeft + dx, top: owner.scrollTop + dy, behavior: 'instant' });
    }
    return true;
  }

  function fullyVisible(node) {
    const box = rect(node);
    const chain = ancestors(node);
    if (!chain || !(box.width > 0 && box.height > 0)) return false;
    for (const owner of chain) {
      const style = getComputedStyle(owner);
      if (!unscaled(style) || style.clipPath !== 'none' || style.maskImage !== 'none') return false;
      const clipsPaint = style.contain.split(' ').some(value => ['paint', 'strict', 'content'].includes(value));
      const [left, top, right, bottom] = clientEdges(owner);
      if ((owner === root || clipsPaint || style.overflowX !== 'visible') && (box.left < left || box.right > right)) return false;
      if ((owner === root || clipsPaint || style.overflowY !== 'visible') && (box.top < top || box.bottom > bottom)) return false;
    }
    const range = document.createRange();
    range.selectNodeContents(node);
    return [...range.getClientRects()].every(fragment => {
      if (fragment.width <= 0 || fragment.height <= 0) return true;
      const xs = [fragment.left + Math.min(1, fragment.width / 4), fragment.right - Math.min(1, fragment.width / 4)];
      const ys = [fragment.top + Math.min(1, fragment.height / 4), fragment.bottom - Math.min(1, fragment.height / 4)];
      return xs.every(x => ys.every(y => node.contains(document.elementFromPoint(x, y))));
    });
  }

  function read() {
    const current = inspect(row);
    return current.selected?.length === selected.length
      && current.selected.every((node, index) => node === selected[index]) ? current.result : null;
  }

  function observe(index) {
    const current = read();
    return current && parts(index).every(fullyVisible) ? current : null;
  }

  // Allow this document's queued scroll events and rendering to settle before
  // observing it again. No focus, pointer or form actions are issued here.
  const paint = () => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  return {
    read,
    observe,
    async reveal(index) {
      if (!read()) return null;
      const nodes = parts(index);
      if (!nodes.every(fullyVisible)) {
        if (!moveTo(nodes)) return null;
        await paint();
      }
      return observe(index);
    },
    async restore() {
      for (const [owner, [left, top]] of anchors) {
        if (owner.isConnected && (left !== owner.scrollLeft || top !== owner.scrollTop)) {
          owner.scrollTo({ left, top, behavior: 'instant' });
        }
      }
      await paint();
      return [...anchors].every(([owner, [left, top]]) =>
        owner.isConnected && left === owner.scrollLeft && top === owner.scrollTop);
    },
  };
})
