function recordedSectionButton(element, expectedLabel) {
    if (!element.isConnected || !element.matches('button[type="button" i], input[type="button" i]') ||
        element.type !== 'button' || element.matches(':disabled') ||
        element.closest('a[href], label, [inert], [aria-disabled="true" i], [aria-hidden="true" i], [aria-readonly="true" i]')) return false;
    const separateAction = ['formaction', 'formmethod', 'formtarget', 'formenctype', 'formnovalidate',
        'command', 'commandfor', 'popovertarget', 'popovertargetaction'];
    if (separateAction.some(name => element.hasAttribute(name))) return false;
    const visible = element.tagName === 'INPUT' ? element.value : element.innerText;
    const normalize = text => (text || '').trim().replace(/\s+/g, ' ').toLowerCase();
    return !!normalize(visible) && normalize(visible) === normalize(expectedLabel);
}
