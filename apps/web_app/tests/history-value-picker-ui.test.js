const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
function element(tag,text=''){return {tagName:tag.toUpperCase(),textContent:text,value:'',children:[],listeners:{},append(...items){this.children.push(...items);},setAttribute(){},addEventListener(type,fn){this.listeners[type]=fn;},dispatchEvent(event){this.listeners[event.type]?.(event);},focus(){}};}
function fixture(){
 const input=element('input'),notices=[],calls=[];input.value='Existing';let resolve,reject;
 const context={element,document:{createElement:element},Option:function(text,value){return {text,value}},Event:class{constructor(type){this.type=type}},CandidateHistory:{choices:()=>[[`history:${'a'.repeat(32)}:employer`,'Employer','Fictional Co']]},notify:text=>notices.push(text),api:(path,body)=>{calls.push({path,body});return new Promise((yes,no)=>{resolve=yes;reject=no;});}};
 vm.createContext(context);
 const helpers=source.split('\n').filter(line=>line.startsWith('function clearFormHistorySource(')||line.startsWith('function linkFormHistorySource(')).join('\n');
 const start=source.indexOf('function profileValuePicker('),end=source.indexOf('\n}',start)+2;
 vm.runInContext(helpers+'\n'+source.slice(start,end),context);
 const picker=context.profileValuePicker({first_name:'Avery'},input,{selector:'#company',label:'Employer',field_type:'text',sensitive:false},'session');
 const [select,button]=picker.children;select.value=`history:${'a'.repeat(32)}:employer`;
 const response={value:{text:'Fictional Co'},history_source:{entry_id:'a'.repeat(32),component:'employer',source_sha256:'b'.repeat(64)}};
 return {input,select,button,calls,notices,response,resolve:value=>resolve(value),reject:error=>reject(error)};
}
(async()=>{
 const x=fixture(),pending=x.button.listeners.click();assert.equal(x.input.value,'Existing');assert.equal(x.button.disabled,true);assert.equal(x.calls[0].path,'/api/form/history-value');x.resolve(x.response);await pending;
 assert.equal(x.input.value,'Fictional Co');assert.equal(x.input._historySource.component,'employer');assert.equal(x.button.disabled,false);
 x.input.value='Edited';x.input.dispatchEvent({type:'input'});x.input.value='Fictional Co';assert.equal(x.input._historySource,undefined);
 const y=fixture(),race=y.button.listeners.click();y.input.value='New manual edit';y.resolve(y.response);await race;assert.equal(y.input.value,'New manual edit');assert.equal(y.input._historySource,undefined);assert.match(y.notices[0],/edits were kept/);
 const z=fixture(),rejected=z.button.listeners.click();z.reject(Error('Profile changed'));await rejected;assert.equal(z.input.value,'Existing');assert.match(z.notices[0],/Profile changed/);
 const a=fixture(),selection=a.button.listeners.click();a.select.value='first_name';a.resolve(a.response);await selection;assert.equal(a.input.value,'Existing');assert.equal(a.input._historySource,undefined);
 x.select.value='first_name';await x.button.listeners.click();assert.equal(x.input.value,'Avery');assert.equal(x.input._historySource,undefined);
 console.log('Saved-history picker resolves current sources locally, preserves edits and selections, and refuses stale reads.');
})().catch(error=>{console.error(error);process.exitCode=1;});
