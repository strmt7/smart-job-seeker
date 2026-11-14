function waypointFieldLabelContext() {
    // Fixed read-only geometry query. No field values, expected labels, model
    // instructions, scrolling, focus or page mutation enter this operation.
    if (!this.isConnected) return null;
    const root = this.getRootNode(), labels = new Set(), regions = [this];
    const references = element => {
        const raw = element.getAttribute('aria-labelledby');
        if (raw === null) return true;
        const ids = raw.trim().split(/\s+/).filter(Boolean);
        if (!ids.length || ids.length > 16 || raw.length > 512 || new Set(ids).size !== ids.length) return false;
        for (const id of ids) {
            const found = root.querySelectorAll('#' + CSS.escape(id));
            if (found.length !== 1) return false;
            labels.add(found[0]);
        }
        return true;
    };
    if (!references(this)) return null;
    if (this.labels) {
        if (this.labels.length > 16) return null;
        for (const label of this.labels) labels.add(label);
    }
    if (!labels.size) return {unavailable:true};
    let ancestor = this.parentElement, depth = 0;
    while (ancestor && depth++ < 64) {
        if (ancestor.localName === 'fieldset') {
            const legends = Array.from(ancestor.children).filter(child => child.localName === 'legend');
            if (legends.length > 1) return null;
            for (const legend of legends) labels.add(legend);
        }
        if (['group', 'radiogroup'].includes(ancestor.getAttribute('role')) && !references(ancestor)) return null;
        ancestor = ancestor.parentElement;
    }
    if (ancestor || labels.size > 24) return null;
    regions.push(...labels);
    let left = Infinity, top = Infinity, right = -Infinity, bottom = -Infinity;
    for (const element of regions) {
        const rect = element.getBoundingClientRect();
        if (!(rect.width > 0 && rect.height > 0)) return null;
        let parent = element, depth = 0;
        while (parent && depth++ < 128) {
            const style = getComputedStyle(parent), bounds = parent.getBoundingClientRect();
            if (style.display === 'none' || style.visibility !== 'visible' || Number(style.opacity) === 0) return null;
            if (/^(hidden|clip|scroll|auto)$/.test(style.overflowX) && (rect.left < bounds.left || rect.right > bounds.right)) return null;
            if (/^(hidden|clip|scroll|auto)$/.test(style.overflowY) && (rect.top < bounds.top || rect.bottom > bounds.bottom)) return null;
            parent = parent.parentElement;
        }
        if (parent) return null;
        left = Math.min(left, rect.left); top = Math.min(top, rect.top);
        right = Math.max(right, rect.right); bottom = Math.max(bottom, rect.bottom);
    }
    if (left < 0 || top < 0 || right > innerWidth || bottom > innerHeight || right-left > 704 || bottom-top > 384) return null;
    left = Math.max(0, left-8); top = Math.max(0, top-8);
    right = Math.min(innerWidth, right+8); bottom = Math.min(innerHeight, bottom+8);
    return {bounds:[left,top,right,top,right,bottom,left,bottom]};
}
