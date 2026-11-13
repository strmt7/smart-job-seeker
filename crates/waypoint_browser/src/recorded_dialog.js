async function() {
    const doc = this, view = doc.defaultView;
    if (!view || doc !== view.document) return null;
    const dialogQuery = 'dialog[open],[role="dialog" i],[role="alertdialog" i],[aria-modal="true" i]';
    const visible = node => {
        if (!node.isConnected || !node.getClientRects().length) return false;
        for (let p = node, n = 0; p; p = p.parentElement) {
            if (++n > 64) return false;
            const style = view.getComputedStyle(p);
            if (style.display === 'none' || style.visibility !== 'visible' || Number(style.opacity) === 0 || p.getAttribute('aria-hidden') === 'true') return false;
        }
        return true;
    };
    const inputs = doc.querySelectorAll('input,select,textarea,[contenteditable="true"]');
    if (inputs.length > 256) return null;
    const values = [];
    for (const node of inputs) {
        const value = node.isContentEditable ? node.innerText : node.value;
        if (typeof value !== 'string' || value.length > 1024 || node.files?.length) return null;
        values.push([node.localName,node.id,node.getAttribute('name'),node.getAttribute('type'),value,node.checked ?? null,
            node instanceof view.HTMLSelectElement ? Array.from(node.selectedOptions).map(option => option.index) : null]);
    }
    const encoded = new view.TextEncoder().encode(JSON.stringify(values));
    if (encoded.length > 65536 || !view.crypto?.subtle) return null;
    const values_sha256 = Array.from(new Uint8Array(await view.crypto.subtle.digest('SHA-256',encoded)), value => value.toString(16).padStart(2,'0')).join('');
    const all = doc.querySelectorAll(dialogQuery);
    if (all.length > 32) return null;
    const dialogs = Array.from(all).filter(visible);
    if (!dialogs.length) return {count:0,values_sha256};
    if (dialogs.length !== 1) return {count:dialogs.length};
    const dialog = dialogs[0];
    const norm = text => (text || '').trim().replace(/\s+/g, ' ');
    const name = node => {
        const ids = node.getAttribute('aria-labelledby');
        if (ids) {
            const refs = ids.trim().split(/\s+/);
            if (!refs.length || refs.length > 8) return '';
            const names = [];
            for (const id of refs) {
                const matches = doc.querySelectorAll('#' + view.CSS.escape(id));
                if (matches.length !== 1 || !visible(matches[0])) return '';
                names.push(norm(matches[0].innerText));
            }
            return names.join(' ');
        }
        return norm(node.getAttribute('aria-label') || (node.tagName === 'INPUT' ? node.value : node.innerText));
    };
    const path = node => {
        const parts = [];
        for (let p = node; p && p.nodeType === 1; p = p.parentElement) {
            if (parts.length >= 64) return null;
            if (p.id && doc.querySelectorAll('#' + view.CSS.escape(p.id)).length === 1) { parts.unshift('#' + view.CSS.escape(p.id)); break; }
            let index = 1;
            for (let s = p.previousElementSibling; s; s = s.previousElementSibling) if (s.localName === p.localName) index++;
            parts.unshift(p.localName + ':nth-of-type(' + index + ')');
        }
        const value = parts.join(' > ');
        return value.length <= 4096 && doc.querySelectorAll(value).length === 1 ? value : null;
    };
    const text = norm(dialog.innerText), title = name(dialog);
    if (!text || text.length > 4000 || title.length > 4000 || dialog.querySelector('iframe,object,embed,input:not([type="button" i]):not([type="submit" i]):not([type="hidden" i]),select,textarea,[contenteditable="true"],[role="checkbox" i],[role="textbox" i]')) return null;
    const nodes = dialog.querySelectorAll('button,input[type="button" i],input[type="submit" i],a[href],[role="button" i]');
    if (!nodes.length || nodes.length > 24) return null;
    const controls = [];
    for (const node of nodes) {
        if (!visible(node)) continue;
        if (controls.length >= 12) return null;
        const selector = path(node), label = name(node), rect = node.getBoundingClientRect();
        if (!selector || !label || label.length > 200 || node.closest(dialogQuery) !== dialog) return null;
        const type = (node.getAttribute('type') || '').toLowerCase();
        const nativeButton = node.tagName === 'BUTTON' && (type === 'button' || !node.hasAttribute('type') && !node.form);
        const inputButton = node.tagName === 'INPUT' && type === 'button';
        const roleButton = !['BUTTON','INPUT','A'].includes(node.tagName) && (node.getAttribute('role') || '').toLowerCase() === 'button';
        const hit = doc.elementFromPoint(rect.x + rect.width/2, rect.y + rect.height/2);
        const available = (nativeButton || inputButton || roleButton) && !node.matches(':disabled') && !node.closest('[aria-disabled="true" i],[inert]')
            && !node.hasAttribute('formaction') && !node.hasAttribute('formmethod') && !node.hasAttribute('formtarget') && !node.closest('a[href]')
            && rect.width > 0 && rect.height > 0 && hit && (hit === node || node.contains(hit));
        controls.push({selector,label,available:!!available});
    }
    if (!controls.length) return null;
    const selector = path(dialog);
    return selector ? {count:1,selector,title,text,controls,values_sha256} : null;
}
