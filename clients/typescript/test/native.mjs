// Opt-in owned-fixture acceptance, invoked by scripts/accept-rc2.py.
import {HostSession,value,one} from '../dist/index.js';
const {DTW_NATIVE_HELPER:helper,DTW_NATIVE_TITLE:title,DTW_NATIVE_TOKEN:token}=process.env;
if(!helper||!title||!token)throw new Error('Run scripts/accept-rc2.py');
const host=await HostSession.start({helper,writeAppWindows:[title]});
try{
 const dw=host.desktop;
 const inv=await dw.observe({budget:{max_results:256,max_output_bytes:65536}});
 const win=one(inv,title);
 const field=one(await dw.find(win.ref,{role:'text_field',name_equals:'内容'}),'内容');
 const submit=one(await dw.find(win.ref,{role:'button',name_equals:'提交'}),'提交');
 const p=dw.plan().focus(field.ref);
 const input=p.bindFocus('input',win.ref);p.press(input,'A',['primary']).type(input,token).invoke(submit.ref);
 const original=await dw.act(p,{},'native-ts-original');
 if((await dw.act(p,{},'native-ts-original')).run_id!==original.run_id)throw new Error('receipt changed');
 if(value((await dw.read(field.ref)).text)!==token)throw new Error('native read differs');
 console.log(JSON.stringify({language:'typescript',run_id:original.run_id,input:original.input,verified:true}));
}finally{await host.close();}
