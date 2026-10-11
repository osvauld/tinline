// tiny check recorder shared by flow scripts
const fs=require('fs');
const R=[];
function mk(page){return async function(name,fn){
 let ok=false,detail='';try{const r=await fn();ok=r===true||(r&&r.ok);detail=r&&r.d!==undefined?r.d:(ok?'':String(r));}catch(e){detail='EXC '+e.message.split('\n')[0]}
 R.push({page,name,ok,detail});console.log((ok?'PASS ':'FAIL ')+page+' · '+name+(detail?' — '+detail:''));return ok}}
function save(f){fs.writeFileSync(f,JSON.stringify(R,null,1))}
module.exports={mk,R,save};
