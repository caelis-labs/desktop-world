'use strict';
const canvas=document.getElementById('chat-canvas'),ctx=canvas.getContext('2d');
const composer=document.getElementById('composer'),statusNode=document.getElementById('chat-status');
let messages=[],offset=420,menu=null,mode=null,anchors={},layoutTimer,width=900,height=610;
async function post(path,data) {
  const response=await fetch(path,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(data)});
  const result=await response.json();if(!response.ok)throw new Error(result.error||`HTTP ${response.status}`);return result;
}
function fail(error){statusNode.textContent=error.message;statusNode.className='error';}
async function refresh(){messages=(await (await fetch('/api/data')).json()).chat.messages;draw();}
function publishLayout(){
  clearTimeout(layoutTimer);layoutTimer=setTimeout(()=>{
    const r=canvas.getBoundingClientRect(),all={};
    for(const [name,point]of Object.entries(anchors))all[name]=[r.x+point[0],r.y+point[1]];
    for(const [name,node]of [['canvas-compose',composer],['canvas-send',document.getElementById('send-message')]]){
      const rect=node.getBoundingClientRect();all[name]=[rect.x+rect.width/2,rect.y+rect.height/2];
    }
    post('/api/layout',{viewport:[innerWidth,innerHeight],anchors:all}).catch(fail);
  },80);
}
function draw(){
  const rect=canvas.getBoundingClientRect();width=rect.width;height=610;
  canvas.width=Math.round(width*devicePixelRatio);canvas.height=Math.round(height*devicePixelRatio);
  ctx.setTransform(devicePixelRatio,0,0,devicePixelRatio,0,0);ctx.fillStyle='#142538';ctx.fillRect(0,0,width,height);
  ctx.fillStyle='#20384e';ctx.fillRect(0,0,width,46);ctx.font='bold 17px system-ui';ctx.fillStyle='#eff7ff';ctx.fillText('# release-operations',20,29);
  anchors={};ctx.save();ctx.beginPath();ctx.rect(0,47,width,height-47);ctx.clip();
  for(let i=0;i<messages.length;i++){
    const y=60+i*61-offset;if(y+52<47||y>height)continue;
    const message=messages[i];ctx.fillStyle=i%2?'#1b3249':'#203a51';ctx.fillRect(48,y,width-72,51);
    ctx.fillStyle=['#65b9bf','#d8a469','#a395d5'][i%3];ctx.beginPath();ctx.arc(25,y+23,13,0,Math.PI*2);ctx.fill();
    ctx.font='11px system-ui';ctx.fillStyle='#a3b8c9';ctx.fillText(`operations / ${String(i+1).padStart(2,'0')}:37`,60,y+15);
    ctx.font='15px system-ui';ctx.fillStyle='#f2f7fc';
    const available=width-130;let visible=message.text;
    while(ctx.measureText(visible).width>available&&visible.length>1)visible=visible.slice(0,-1);
    ctx.fillText(visible,60,y+36);
    if(message.pinned){ctx.fillStyle='#ffd76d';ctx.fillText('◆',width-46,y+32);}
    if(message.id==='release-137'&&y+25>47&&y+25<height)anchors['canvas-message-137']=[Math.min(width-80,230),y+29];
  }
  ctx.restore();
  // The menu and message bodies are pixels only; no parallel DOM labels.
  if(menu){ctx.fillStyle='#f5f8fc';ctx.shadowColor='#0008';ctx.shadowBlur=10;ctx.fillRect(menu.x,menu.y,150,114);ctx.shadowBlur=0;
    ['Pin','Edit','Reply'].forEach((label,i)=>{ctx.fillStyle='#18324a';ctx.font='16px system-ui';ctx.fillText(label,menu.x+17,menu.y+25+i*36);anchors['canvas-'+label.toLowerCase()]=[menu.x+75,menu.y+18+i*36];});}
  publishLayout();
}
canvas.addEventListener('wheel',event=>{event.preventDefault();offset=Math.max(0,Math.min(Math.max(0,messages.length*61-530),offset+event.deltaY));menu=null;draw();},{passive:false});
canvas.addEventListener('contextmenu',event=>{event.preventDefault();const rect=canvas.getBoundingClientRect(),x=event.clientX-rect.x,y=event.clientY-rect.y;
  if(y<47)return;const index=Math.floor((y+offset-60)/61);if(!messages[index])return;
  menu={id:messages[index].id,x:Math.max(0,Math.min(x,width-155)),y:Math.max(47,Math.min(y,height-120))};draw();
});
canvas.addEventListener('click',async event=>{
  if(!menu)return;const rect=canvas.getBoundingClientRect(),x=event.clientX-rect.x,y=event.clientY-rect.y,selected=menu;menu=null;
  if(x>=selected.x&&x<=selected.x+150&&y>=selected.y&&y<selected.y+108){
    const action=['pin','edit','reply'][Math.floor((y-selected.y)/36)];
    if(action==='pin'){try{await post('/api/chat',{action,id:selected.id});statusNode.textContent='Message pinned';await refresh();}catch(error){fail(error);}}
    else{mode={action,id:selected.id};composer.value=action==='edit'?messages.find(m=>m.id===selected.id).text:'';statusNode.textContent=action==='edit'?'Editing message':'Replying to message';composer.focus();}
  }draw();
});
document.getElementById('composer-bar').onsubmit=async event=>{event.preventDefault();if(!composer.value.trim())return;
  const payload={...(mode||{action:'send'}),text:composer.value};
  try{await post('/api/chat',payload);composer.value='';mode=null;statusNode.textContent='Message saved';await refresh();}catch(error){fail(error);}
};
addEventListener('resize',draw);addEventListener('scroll',publishLayout,true);refresh().catch(fail);
