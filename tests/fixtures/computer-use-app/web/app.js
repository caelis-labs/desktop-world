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
let layoutTimer, invalidReviews=0, catalogueReviews={};
async function observe() {
  const form=$('record-form'); if(!form)return;
  const values=Object.fromEntries(new FormData(form));
  values.approved=form.elements.approved.checked?'on':'';
  const scrolls={document:[scrollX,scrollY]};
  for(const [name,node] of [['inner',$('catalogue')],['outer',document.querySelector('.outer-scroll')]])scrolls[name]=[node.scrollLeft,node.scrollTop];
  await post('/api/observation',{review:{open:$('record-dialog').open,invalid:invalidReviews,values},scrolls});
}
function scheduleLayout() {
  clearTimeout(layoutTimer);
  layoutTimer=setTimeout(()=>run(async()=>{
    const anchors={};
    for(const [name,id] of Object.entries({'web-note':'web-note','web-commit':'commit-note','web-reference':'web-reference','web-checkpoint':'checkpoint-137','web-catalogue':'load-catalogue'})) {
      const node=$(id); if(!node)continue; const rect=node.getBoundingClientRect();
      if(rect.width && rect.height)anchors[name]=[rect.x+rect.width/2,rect.y+rect.height/2];
    }
    for(const [name,node] of [['review-note',$('record-form')?.elements.note],
      ['review-number',$('record-form')?.elements.number],['review-approved',$('record-form')?.elements.approved],
      ['review-date',$('record-form')?.elements.date],['catalogue-search',$('search')]]) {
      if(!node)continue;const r=node.getBoundingClientRect();
      if(r.width && r.height)anchors[name]=[r.x+(name==='review-date'?Math.min(16,r.width/2):r.width/2),r.y+r.height/2];
    }
    for(const [name,node] of [['scroll-inner',$('catalogue')],['scroll-outer',document.querySelector('.outer-scroll')]]) {
      if(!node)continue;const r=node.getBoundingClientRect();
      const top=Math.max(0,r.top),bottom=Math.min(innerHeight,r.bottom);
      const left=Math.max(0,r.left),right=Math.min(innerWidth,r.right);
      if(bottom<=top || right<=left)continue;
      anchors[name]=name==='scroll-outer'?[left+Math.min(4,(right-left)/2),(top+bottom)/2]:[(left+Math.min(right,left+500))/2,(top+bottom)/2];
    }
    await post('/api/layout',{viewport:[innerWidth,innerHeight],anchors});
    await observe();
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
    const reviewed=catalogueReviews[String(i)]===true || i%3===0; const title=i===137?'Audit checkpoint 137':`Archive ledger ${String(i).padStart(3,'0')}`;
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
      button.onclick=()=>run(async()=>{
        if(action==='Reviewed'){
          await post('/api/catalogue-review',{id:i});catalogueReviews[String(i)]=true;renderCatalogue();
        }
        await post('/api/event',{action,target:String(i)});status(`${action}: ledger ${i}`);
      });nested.append(button);
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
  catalogueReviews=data.catalogue_reviews;
  if(data.catalogue_loaded)renderCatalogue();
  $('record-form').addEventListener('invalid',()=>{invalidReviews++;scheduleLayout();},true);
  $('record-form').addEventListener('input',scheduleLayout);
  $('record-dialog').addEventListener('close',scheduleLayout);
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
