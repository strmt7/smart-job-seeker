async function(specs, ...nodes) {
  const selections = specs.map(({ expected, requireForm }, index) => {
    const input = nodes[2 * index];
    const owner = nodes[2 * index + 1];
    const available = () => input.isConnected && input instanceof HTMLInputElement
      && input.type === 'file' && !input.matches(':disabled')
      && !input.closest('[aria-disabled="true" i], [aria-readonly="true" i]')
      && (!requireForm || (input.ownerDocument === document && input.form === owner));
    const current = () => {
      if (!available() || !input.files?.length) return 'not_retained';
      return (expected.length === 1 || input.multiple) && input.files.length === expected.length
        && expected.every((file, i) => input.files[i].name === file.name && input.files[i].size === file.size)
        ? 'verified' : 'different';
    };
    const status = current();
    return { input, expected, current, status, files: status === 'verified' ? Array.from(input.files) : [] };
  });
  for (const selection of selections) {
    if (selection.status !== 'verified') continue;
    if (!globalThis.crypto?.subtle) {
      selection.status = 'unavailable';
      continue;
    }
    try {
      for (const [index, file] of selection.files.entries()) {
        const digest = await crypto.subtle.digest('SHA-256', await file.arrayBuffer());
        const hex = Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, '0')).join('');
        if (hex !== selection.expected[index].sha256) {
          selection.status = 'different';
          break;
        }
      }
    } catch (_) {
      selection.status = 'unavailable';
    }
  }
  // Recheck every hashed selection together after all asynchronous byte reads.
  return selections.map(({ input, current, status, files }) => {
    if (status !== 'verified') return { status };
    const final = current();
    if (final !== 'verified') return { status: final };
    if (files.some((file, index) => input.files[index] !== file)) return { status: 'different' };
    return { status: 'verified', accept: input.accept };
  });
}
