/* Shared career-history editor. Candidate values are always rendered as text. */
(() => {
  'use strict';
  const VERSION = 1, LIMIT = 20;
  const groups = {
    employment: {title:'Employment', add:'Add employment', empty:'Add roles you want to reuse in application answers.', fields:[['employer','Employer',true],['title','Job title',true],['location','Location']]},
    education: {title:'Education', add:'Add education', empty:'Add qualifications you want to reuse in application answers.', fields:[['institution','School or university',true],['qualification','Qualification',true],['field_of_study','Field of study'],['location','Location']]}
  };
  const empty = () => ({version:VERSION, employment:[], education:[]});
  const node = (tag, text, className) => {
    const element = document.createElement(tag);
    if (text) element.textContent = text;
    if (className) element.className = className;
    return element;
  };
  function parse(raw) {
    if (!raw) return empty();
    if (typeof raw !== 'string' || new TextEncoder().encode(raw).length > 65536) throw new Error('Career history exceeds its storage limit.');
    const value = JSON.parse(raw);
    if (!value || value.version !== VERSION || !Array.isArray(value.employment) || !Array.isArray(value.education)) throw new Error('Saved career history has an unsupported format.');
    const ids = new Set();
    for (const key of Object.keys(groups)) {
      if (value[key].length > LIMIT) throw new Error('Career history has too many entries.');
      for (const record of value[key]) {
        if (!record || !/^[0-9a-f]{32}$/.test(record.id) || ids.has(record.id)) throw new Error('A history entry has an invalid identity.');
        ids.add(record.id);
        for (const field of [...groups[key].fields.map(([key]) => key), 'start_date', 'end_date', 'description']) {
          if (record[field] !== undefined && typeof record[field] !== 'string') throw new Error('A history entry contains unreadable text.');
        }
        for (const field of ['current','reviewed']) if (record[field] !== undefined && typeof record[field] !== 'boolean') throw new Error('A history entry contains an invalid review status.');
      }
    }
    return value;
  }
  function choices(profile) {
    let history;
    try { history = parse(profile?.career_history || ''); } catch (_) { return []; }
    const result = [];
    for (const [key, config] of Object.entries(groups)) for (const record of history[key]) {
      if (record.reviewed !== true) continue;
      const name = key === 'employment' ? [record.title, record.employer].filter(Boolean).join(' — ') : [record.qualification, record.institution].filter(Boolean).join(' — ');
      for (const [field,label] of [...config.fields,['start_date','Start date'],['end_date','End date'],['description',key==='employment'?'Responsibilities':'Details']]) {
        const value = record[field];
        if (typeof value === 'string' && value.trim() && new TextEncoder().encode(value).length <= 4000) result.push([`history:${record.id}:${field}`, `${config.title} · ${name} · ${label}`, value]);
      }
    }
    return result;
  }
  const host = document.querySelector('#candidate-history');
  const hidden = document.querySelector('#career-history-value');
  let history = empty();
  function saveDraft() {
    hidden.value = JSON.stringify(history);
    hidden.dispatchEvent(new Event('input', {bubbles:true}));
  }
  function button(text, action) {
    const result = node('button', text);
    result.type = 'button';
    result.addEventListener('click', action);
    return result;
  }
  function render() {
    if (!host || !hidden) return;
    host.replaceChildren();
    host.append(node('h3','Career history'), node('p','Save your employment and education details for reviewed application answers. Blank dates stay unknown; use YYYY or YYYY-MM.', 'footnote'));
    for (const [key, config] of Object.entries(groups)) {
      const section = node('section', null, 'history-section');
      section.dataset.historyGroup=key;
      section.setAttribute('aria-label', config.title);
      const head = node('div', null, 'history-section-heading');
      head.append(node('h4',config.title));
      const add = button(config.add, () => {
        if (history[key].length >= LIMIT) return;
        const bytes = new Uint8Array(16);
        crypto.getRandomValues(bytes);
        const record = {id:Array.from(bytes,b=>b.toString(16).padStart(2,'0')).join(''), current:false, reviewed:false};
        for (const field of [...config.fields.map(([name])=>name),'start_date','end_date','description']) record[field]='';
        history[key].push(record);
        saveDraft(); render();
        host.querySelector(`[data-history-id="${record.id}"] input`)?.focus();
      });
      add.disabled = history[key].length >= LIMIT;
      head.append(add); section.append(head);
      if (!history[key].length) section.append(node('p',config.empty,'history-empty'));
      history[key].forEach((record,index) => {
        const card = node('fieldset', null, 'history-entry');
        card.dataset.historyId = record.id;
        const title = node('legend');
        const updateTitle=()=>{const name=key==='employment'?[record.title,record.employer].filter(Boolean).join(' at '):[record.qualification,record.institution].filter(Boolean).join(' at ');title.textContent=name?Array.from(name).slice(0,100).join(''):`${config.title==='Employment'?'Role':'Qualification'} ${index+1}`;title.title=name;};
        updateTitle();
        card.append(title);
        const tools = node('div',null,'history-entry-tools');
        const up = button('Move up',()=>{if(index<1)return;[history[key][index-1],history[key][index]]=[record,history[key][index-1]];saveDraft();render();host.querySelector(`[data-history-id="${record.id}"] button:not(:disabled)`)?.focus();});
        const down = button('Move down',()=>{if(index+1>=history[key].length)return;[history[key][index+1],history[key][index]]=[record,history[key][index+1]];saveDraft();render();host.querySelector(`[data-history-id="${record.id}"] button:not(:disabled)`)?.focus();});
        up.disabled=index===0; down.disabled=index+1===history[key].length;
        let confirmed=false;
        const remove=button('Remove',()=>{if(!confirmed){confirmed=true;remove.textContent='Confirm removal';remove.setAttribute('aria-label',`Confirm removal of ${config.title.toLowerCase()} entry ${index+1}`);return;}history[key].splice(index,1);saveDraft();render();host.querySelector(`[data-history-group="${key}"] .history-section-heading button`)?.focus();});
        remove.setAttribute('aria-label',`Remove ${config.title.toLowerCase()} entry ${index+1}`);
        tools.append(up,down,remove);card.append(tools);
        const fields=node('div',null,'history-fields');
        const reviewLabel=node('label',null,'history-check'),review=document.createElement('input');
        review.type='checkbox'; review.checked=record.reviewed===true;
        reviewLabel.append(review,document.createTextNode('I have checked these details'));
        const status=node('span',review.checked?'Checked':'Needs review','history-review-status');
        status.setAttribute('aria-live','polite');
        const changed=()=>{record.reviewed=false;review.checked=false;status.textContent='Needs review';updateTitle();saveDraft();};
        function field(name,label,required=false,multiline=false) {
          const wrapper=node('label',label,multiline?'history-description':null),input=document.createElement(multiline?'textarea':'input');
          input.value=record[name]||'';input.required=required;input.maxLength=multiline?4000:500;
          input.setAttribute('data-history-field',name);
          if (name==='start_date'||name==='end_date') { input.placeholder='YYYY or YYYY-MM';input.pattern='[0-9]{4}(-[0-9]{2})?'; input.maxLength=7; }
          input.addEventListener('input',()=>{record[name]=input.value;changed();});
          wrapper.append(input);fields.append(wrapper);return input;
        }
        for(const [name,label,required] of config.fields) field(name,label,required);
        field('start_date','Start date');const end=field('end_date','End date');
        end.disabled=record.current===true;
        const currentLabel=node('label',null,'history-check'),current=document.createElement('input'),currentNote=node('p',null,'footnote');
        currentNote.hidden=true;currentNote.setAttribute('role','status');
        current.type='checkbox';current.checked=record.current===true;
        current.addEventListener('change',()=>{if(current.checked&&record.end_date){current.checked=false;currentNote.textContent='Clear the end date before marking this entry as ongoing.';currentNote.hidden=false;end.focus();return;}currentNote.hidden=true;record.current=current.checked;end.disabled=current.checked;changed();});
        end.addEventListener('input',()=>{currentNote.hidden=true;});
        currentLabel.append(current,document.createTextNode('Ongoing'));fields.append(currentLabel,currentNote);
        field('description',key==='employment'?'Responsibilities and achievements':'Additional details',false,true);
        review.addEventListener('change',()=>{record.reviewed=review.checked;status.textContent=review.checked?'Checked':'Needs review';saveDraft();});
        card.append(fields,reviewLabel,status);section.append(card);
      });
      section.append(node('p',`${history[key].length} of ${LIMIT} entries`,'footnote'));host.append(section);
    }
    host.append(node('p','Only checked entries appear in saved-value choices. Choosing a value prepares an answer for review; it does not fill a website or submit an application.','footnote'));
  }
  function load(raw) {
    if (!host || !hidden) return;
    hidden.value=typeof raw==='string'?raw:JSON.stringify(raw??null);
    try { history=parse(hidden.value);render(); }
    catch (_) { host.replaceChildren(node('h3','Career history'),node('p','Saved history could not be read. Its original value has been kept. Reload before making changes.','review-label')); }
  }
  globalThis.CandidateHistory=Object.freeze({load,choices,parse});
  if(host&&hidden) {hidden.addEventListener('change',()=>load(hidden.value));load(hidden.value);}
})();
