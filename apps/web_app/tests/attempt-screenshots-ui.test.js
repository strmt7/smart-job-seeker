const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const line=source.split('\n').find(value=>value.startsWith('async function loadAttemptScreenshots('));
assert.ok(line);
function node(tag,text='',className=''){return {tag,textContent:text,className,children:[],listeners:{},append(...values){this.children.push(...values)},replaceChildren(...values){this.children=values},addEventListener(event,fn){this.listeners[event]=fn}}}
const calls=[],panel=node('div');
const context={element:node,document:{createElement:tag=>node(tag)},api:async(path,body)=>{calls.push({path,body});return path.endsWith('/screenshots')?{items:['0001.png'],contexts:{'0001.png':{type:'fields_filled',time:null,field_label:'Fictional name'}},context_status:'available',unreadable:0,truncated:false,next_after:null}:{name:'0001.png',image_base64:'aGVsbG8='}}};
const helper=source.split('\n').find(value=>value.startsWith('async function loadSavedCaptureReview('));
vm.runInNewContext(helper,context);
const load=vm.runInNewContext(`(${line})`,context);
(async()=>{await load(panel,'job','attempt');assert.equal(calls.length,1);const button=panel.children.find(item=>item.tag==='button');assert.equal(button.textContent,'View capture 0001 · After filling · Fictional name');await button.listeners.click();assert.equal(calls.length,2);assert.equal(calls[1].body.name,'0001.png');const view=panel.children.find(item=>item.className==='attempt-screenshot-view');assert.equal(view.children[0].src,'data:image/png;base64,aGVsbG8=');assert.match(panel.children[0].textContent,/do not prove/);console.log('Attempt screenshot viewer checks passed.');})().catch(error=>{console.error(error);process.exitCode=1});

(async()=>{const review=node('div'),reviewContext={element:node,api:async()=>({status:'complete',visual_review:{observations:[{label:'Name',visible_value:'Fixture'}],comparisons:[{label:'Name',status:'matches_transcription'}]}})};vm.runInNewContext(helper,reviewContext);await reviewContext.loadSavedCaptureReview(review,'job','attempt','0123456789abcdef0123456789abcdef');assert.match(review.children[0].textContent,/before later live-value/);assert.equal(review.children[1].textContent,'Name: Fixture');assert.match(review.children[2].textContent,/Matches transcription/);})().catch(error=>{console.error(error);process.exitCode=1});
