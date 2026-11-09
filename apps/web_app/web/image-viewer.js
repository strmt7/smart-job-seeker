/* One viewer for local document pages and captured evidence. Never edits evidence. */
globalThis.SmartJobseekerImageViewer = (() => {
  const MAX_SOURCE = 32 * 1024 * 1024, MAX_PIXELS = 32 * 1024 * 1024;
  const controllers=new WeakMap();
  const clamp = (value, low, high) => Math.min(high, Math.max(low, value));
  function safeSource(value) {
    return typeof value === 'string' && value.length <= MAX_SOURCE && /^data:image\/(?:png|jpeg|webp);base64,[A-Za-z0-9+/]+={0,2}$/i.test(value);
  }
  function validSize(width, height) {
    return Number.isInteger(width) && Number.isInteger(height) && width > 0 && height > 0 && width <= 32768 && height <= 32768 && width * height <= MAX_PIXELS;
  }
  function fit(width, height, viewportWidth, viewportHeight) {
    if (!validSize(width, height) || !Number.isFinite(viewportWidth) || !Number.isFinite(viewportHeight) || viewportWidth <= 0 || viewportHeight <= 0) return null;
    return Math.min(1, viewportWidth / width, viewportHeight / height);
  }
  function position(value, viewport, rendered) {
    if (![value, viewport, rendered].every(Number.isFinite) || viewport <= 0 || rendered <= 0) return 0;
    return rendered <= viewport ? (viewport - rendered) / 2 : clamp(value, viewport - rendered, 0);
  }
  function zoom(state, next, anchorX, anchorY) {
    if (!state || ![next, anchorX, anchorY, state.scale, state.x, state.y, state.width, state.height, state.vw, state.vh].every(Number.isFinite) || state.scale <= 0) return null;
    const fitted = fit(state.width, state.height, state.vw, state.vh);
    if (fitted === null) return null;
    const scale = clamp(next, Math.min(.1, fitted), 8), ratio = scale / state.scale;
    return {...state, scale, x:position(anchorX - (anchorX-state.x)*ratio,state.vw,state.width*scale), y:position(anchorY - (anchorY-state.y)*ratio,state.vh,state.height*scale)};
  }
  function install(document) {
    if(controllers.has(document))return controllers.get(document);
    const originals=new WeakMap(), attributes=['role','tabindex','aria-haspopup','aria-label','title'];
    let dialog, stage, picture, title, status, fitButton, actualButton, minus, plus, readout;
    let state = null, opener = null, generation = 0, drag = null, previousOverflow = '', holdingPage = false;
    const create = (tag, text, className) => {
      const node = document.createElement(tag);
      if (text) node.textContent = text;
      if (className) node.className = className;
      return node;
    };
    function releaseDrag() {
      const previous=drag; drag=null;
      if (previous && stage?.hasPointerCapture(previous.id)) stage.releasePointerCapture(previous.id);
      stage?.classList.remove('is-panning');
    }
    function cleanup() {
      if(!holdingPage)return;
      holdingPage=false; generation++; releaseDrag(); state=null;
      picture.hidden=true; picture.removeAttribute('src'); picture.removeAttribute('style');
      document.body.style.overflow=previousOverflow;
      const source=opener; opener=null; if(source?.isConnected)source.focus({preventScroll:true});
    }
    function closeViewer() { if(dialog?.open){ dialog.close(); cleanup(); } }
    function paint() {
      if (!state || !dialog.open) return;
      state.x = position(state.x,state.vw,state.width*state.scale);
      state.y = position(state.y,state.vh,state.height*state.scale);
      picture.style.transform = `translate(${state.x}px,${state.y}px) scale(${state.scale})`;
      readout.textContent = `${Math.round(state.scale*1000)/10}%`;
      fitButton.disabled=false; actualButton.disabled=false;
      fitButton.setAttribute('aria-pressed',String(state.mode==='fit'));
      actualButton.setAttribute('aria-pressed',String(state.mode==='actual'));
      minus.disabled = state.scale <= Math.min(.1,fit(state.width,state.height,state.vw,state.vh))+1e-7;
      plus.disabled = state.scale >= 8-1e-7;
      stage.classList.toggle('is-pannable',state.width*state.scale>state.vw+1 || state.height*state.scale>state.vh+1);
    }
    function fitImage() {
      if (!state) return;
      state.scale = fit(state.width,state.height,state.vw,state.vh);
      state.mode = 'fit'; state.x = 0; state.y = 0; paint();
    }
    function magnify(scale, x=state?.vw/2, y=state?.vh/2, mode='manual') {
      const next = zoom(state,scale,x,y);
      if (next) { state = {...next, mode}; paint(); }
    }
    function resize() {
      if (!state || !dialog.open) return;
      const vw = stage.clientWidth, vh = stage.clientHeight;
      if (vw <= 0 || vh <= 0) return;
      const cx = (state.vw/2-state.x)/state.scale, cy = (state.vh/2-state.y)/state.scale;
      state.vw = vw; state.vh = vh;
      if (state.mode==='fit') fitImage();
      else { state.x=vw/2-cx*state.scale; state.y=vh/2-cy*state.scale; paint(); }
    }
    function build() {
      dialog = create('dialog',null,'image-viewer'); dialog.id = 'image-viewer'; dialog.setAttribute('aria-labelledby','image-viewer-title');
      const heading = create('div',null,'image-viewer-heading');
      title = create('h2'); title.id = 'image-viewer-title';
      const close = create('button','Close','secondary-button'); close.type='button'; close.addEventListener('click',closeViewer); heading.append(title,close);
      const tools = create('div',null,'image-viewer-tools'); tools.setAttribute('role','group'); tools.setAttribute('aria-label','Image zoom');
      const button = (text,label,action) => { const node=create('button',text,'secondary-button'); node.type='button'; node.setAttribute('aria-label',label); node.addEventListener('click',action); return node; };
      fitButton=button('Fit','Fit image',fitImage); actualButton=button('100%','Actual image size',()=>magnify(1,undefined,undefined,'actual'));
      minus=button('−','Zoom out',()=>{if(state)magnify(state.scale/1.25);}); plus=button('+','Zoom in',()=>{if(state)magnify(state.scale*1.25);});
      readout=create('output'); readout.setAttribute('aria-label','Zoom level'); tools.append(fitButton,actualButton,minus,readout,plus);
      stage=create('div',null,'image-viewer-stage'); stage.setAttribute('aria-label','Image inspection area');
      picture=create('img'); picture.draggable=false; picture.alt=''; picture.hidden=true; stage.append(picture);
      status=create('p','Drag to move · Scroll to zoom','image-viewer-status'); status.setAttribute('role','status');
      dialog.append(heading,tools,stage,status); document.body.append(dialog);
      dialog.addEventListener('close',()=>{if(!dialog.open)cleanup();});
      stage.addEventListener('wheel',event=>{ if(!state || event.ctrlKey || event.metaKey)return; event.preventDefault(); const box=stage.getBoundingClientRect(); magnify(state.scale*(event.deltaY<0?1.15:1/1.15),event.clientX-box.left,event.clientY-box.top); },{passive:false});
      stage.addEventListener('pointerdown',event=>{ if(!state || event.button!==0)return; releaseDrag(); drag={id:event.pointerId,x:event.clientX,y:event.clientY,left:state.x,top:state.y}; stage.setPointerCapture(event.pointerId); stage.classList.add('is-panning'); event.preventDefault(); });
      stage.addEventListener('pointermove',event=>{ if(!drag || event.pointerId!==drag.id || !state)return; state.x=drag.left+event.clientX-drag.x; state.y=drag.top+event.clientY-drag.y; paint(); });
      for(const type of ['pointerup','pointercancel','lostpointercapture'])stage.addEventListener(type,releaseDrag);
      stage.addEventListener('dblclick',event=>{ if(!state)return; event.preventDefault(); if(state.mode==='fit')magnify(1);else fitImage(); });
      new ResizeObserver(resize).observe(stage);
      return close;
    }
    async function open(source) {
      const src=source.src;
      if(!safeSource(src) || !source.complete || !validSize(source.naturalWidth,source.naturalHeight))return false;
      const close=dialog ? dialog.querySelector('.image-viewer-heading button') : build();
      generation++; const current=generation; opener=source; state=null; picture.hidden=true;
      for(const button of [fitButton,actualButton,minus,plus])button.disabled=true;
      title.textContent=(source.alt || 'Image preview').slice(0,200); status.textContent='Loading image…';
      if(!dialog.open){ previousOverflow=document.body.style.overflow; holdingPage=true; document.body.style.overflow='hidden'; dialog.showModal(); }
      close.focus({preventScroll:true});
      picture.src=src;
      try {
        await picture.decode();
        if(current!==generation || !dialog.open)return false;
        if(!validSize(picture.naturalWidth,picture.naturalHeight))throw Error('Image exceeds the inspection size limit');
        state={width:picture.naturalWidth,height:picture.naturalHeight,vw:stage.clientWidth,vh:stage.clientHeight,scale:1,x:0,y:0,mode:'fit'};
        picture.style.width=`${state.width}px`; picture.style.height=`${state.height}px`; picture.hidden=false;
        fitImage(); status.textContent='Drag to move · Scroll to zoom'; return true;
      } catch (_) { if(current===generation && dialog.open)status.textContent='This image could not be displayed. Close the viewer and try again.'; return false; }
    }
    function enhance(image) {
      if(image.closest('a,button,dialog') || !safeSource(image.src)){
        const saved=originals.get(image);
        if(saved){ image.classList.remove('inspectable-image');for(const [key,value] of saved){if(value===null)image.removeAttribute(key);else image.setAttribute(key,value);}originals.delete(image); }
        return;
      }
      if(!originals.has(image)){
        if(image.hasAttribute('role') && image.getAttribute('role')!=='img')return;
        originals.set(image,attributes.map(key=>[key,image.getAttribute(key)]));
      }
      image.classList.add('inspectable-image'); image.setAttribute('role','button'); image.tabIndex=0;
      image.setAttribute('aria-haspopup','dialog'); image.setAttribute('aria-label',`Open full size: ${image.alt || 'image preview'}`); image.title='Open full size';
    }
    function visit(node) { if(node.nodeType!==1)return; if(node.tagName==='IMG')enhance(node); node.querySelectorAll?.('img').forEach(enhance); }
    visit(document.body);
    const observer=new MutationObserver(records=>{ for(const record of records){ if(record.type==='attributes')enhance(record.target); else record.addedNodes.forEach(visit); } });
    observer.observe(document.body,{subtree:true,childList:true,attributes:true,attributeFilter:['src','alt']});
    document.addEventListener('click',event=>{ const image=event.target.closest?.('img.inspectable-image'); if(image && event.button===0){ event.preventDefault(); void open(image); } });
    document.addEventListener('keydown',event=>{ if(['Enter',' '].includes(event.key) && event.target.matches?.('img.inspectable-image')){ event.preventDefault(); void open(event.target); } });
    const controller={open,close:closeViewer};
    controllers.set(document,controller);return controller;
  }
  return {safeSource,validSize,fit,position,zoom,install};
})();
if(typeof document!=='undefined' && document.body)SmartJobseekerImageViewer.install(document);
