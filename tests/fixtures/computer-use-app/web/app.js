'use strict';
const $ = id => document.getElementById(id);
async function post(path, data) {
  const response = await fetch(path, {method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify(data)});
  const result = await response.json();
  if (!response.ok) throw new Error(result.error || `HTTP ${response.status}`);
  return result;
}
function status(message, error=false) { const node=$('status'); node.textContent=message; node.className=error?'error':''; }
async function run(action) {try {await action();} catch(error) {status(error.message,true);}}
let layoutTimer;
function scheduleLayout() {
  clearTimeout(layoutTimer);
  layoutTimer=setTimeout(()=>run(async()=>{
    const anchors={};
    for(const [name,id] of Object.entries({'web-note':'web-note','web-commit':'commit-note','web-reference':'web-reference','web-checkpoint':'checkpoint-137','web-catalogue':'load-catalogue'})) {
      const node=$(id); if(!node)continue; const rect=node.getBoundingClientRect();
      if(rect.width && rect.height)anchors[name]=[rect.x+rect.width/2,rect.y+rect.height/2];
    }
    await post('/api/layout',{viewport:[innerWidth,innerHeight],anchors});
  }),100);
}
async function navigation() {
  const config=await (await fetch('/api/config')).json();
  if(config.workbench_url && $('workbench')) {
    for(const [label,path] of [['Inventory',''],['Reliability','reliability'],['Workbench vision','vision']]) {
      const link=document.createElement('a');link.textContent=label;link.href=config.workbench_url.replace(/\/$/,'')+'/'+path;$('workbench').append(link);
    }
  }
}
function renderCatalogue() {
  const query=$('search').value.toLowerCase(),filter=$('filter').value,page=$('page').value;
  const fragment=document.createDocumentFragment();
  for(let i=1;i<=240;i++) {
    const reviewed=i%3===0; const title=i===137?'Audit checkpoint 137':`Archive ledger ${String(i).padStart(3,'0')}`;
    if(query && !title.toLowerCase().includes(query))continue;
    if(filter!=='All' && (filter==='Reviewed')!==reviewed)continue;
    if(page!=='all' && Math.ceil(i/80)!==Number(page))continue;
    const row=document.createElement('article');row.className='catalogue-row';
    const label=document.createElement('span');label.textContent=`Lane ${i%12} / group ${Math.ceil(i/20)}`;
    const link=document.createElement('a');link.href='#catalogue';link.textContent=title;
    if(i===137)link.id='checkpoint-137';
    link.addEventListener('click',event=>{event.preventDefault();run(async()=>{await post(i===137?'/api/checkpoint':'/api/event',i===137?{id:137}:{action:'open-ledger',target:String(i)});status(`Opened ${title}`);});});
    const nested=document.createElement('section');nested.setAttribute('aria-label',`Review controls ${i}`);
    for(const action of ['Edit','Reviewed']) {
      const button=document.createElement('button');button.textContent=action;
      button.onclick=()=>run(async()=>{await post('/api/event',{action,target:String(i)});status(`${action}: ledger ${i}`);});nested.append(button);
    }
    const detail=document.createElement('span');detail.textContent=reviewed?'Reviewed':'Pending';
    row.append(label,link,nested,detail);fragment.append(row);
  }
  $('catalogue').replaceChildren(fragment);scheduleLayout();
}
async function init() {
  await navigation();
  if($('vision-form')) {
    $('vision-form').onsubmit=event=>{event.preventDefault();run(async()=>{const result=await post('/api/vision',{value:$('vision-value').value});status(result.passed?'Image verified':'Image did not match',!result.passed);});};return;
  }
  const data=await (await fetch('/api/data')).json();
  $('web-note').value=data.note;$('web-reference').value=data.reference;$('capacity').value=data.slider;
  $('commit-note').onclick=()=>run(async()=>{await post('/api/note',{note:$('web-note').value});status('Web note committed');scheduleLayout();});
  $('save-reference').onclick=()=>run(async()=>{await post('/api/reference',{reference:$('web-reference').value});status('Reference saved');});
  $('load-catalogue').onclick=()=>run(async()=>{await post('/api/catalogue',{});renderCatalogue();status('Catalogue loaded: 240 rows, 720 links and controls');});
  for(const id of ['search','filter','page'])$(id).addEventListener(id==='search'?'input':'change',renderCatalogue);
  if(data.catalogue_loaded)renderCatalogue();
  $('open-record').onclick=()=>{$('record-dialog').showModal();scheduleLayout();};
  $('cancel-record').onclick=()=>$('record-dialog').close();
  $('record-form').onsubmit=event=>{event.preventDefault();const form=new FormData(event.target);
    const payload={note:form.get('note'),number:Number(form.get('number')),approved:form.get('approved')==='on',priority:form.get('priority'),date:form.get('date')};
    $('save-record').disabled=true;$('record-status').textContent='Saving…';
    setTimeout(async()=>{try{await post('/api/record',payload);$('record-dialog').close();status('Review saved');}catch(error){$('record-status').textContent=error.message;}finally{$('save-record').disabled=false;}},400);
  };
  $('capacity').oninput=()=>$('capacity-value').textContent=$('capacity').value;
  $('capacity').onchange=()=>run(async()=>{await post('/api/slider',{value:Number($('capacity').value)});status('Capacity updated');});
  addEventListener('resize',scheduleLayout);addEventListener('scroll',scheduleLayout,true);scheduleLayout();
}
init().catch(error=>status(error.message,true));
