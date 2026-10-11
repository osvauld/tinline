const L=require('./lib'),C=require('./check');
const chk=C.mk('group-polish');const sleep=(p,ms)=>p.waitForTimeout(ms);
const foc=p=>p.evaluate(()=>{const a=document.activeElement;return a&&a!==document.body?(a.id||a.getAttribute('aria-label')||(a.textContent||'').trim().slice(0,22)||a.tagName):'BODY'});
const dopen=p=>p.evaluate(()=>!!document.querySelector('dialog[open]'));
const shot=(p,n)=>p.screenshot({path:'screenshots/group-polish-'+n+'.png'});
const idleText=p=>p.evaluate(()=>document.querySelector('#msgs').innerText);
const fit=async(p,tag)=>chk(tag+': no clip/overflow',async()=>{const r=await p.evaluate(()=>{const m=document.querySelector('#msgs').getBoundingClientRect();const bad=[];document.querySelectorAll('#msgs .msg,#msgs .mo,.comp textarea,.comp .send').forEach(e=>{const b=e.getBoundingClientRect();if(b.width&&(b.left<m.left-0.5||b.right>m.right+0.5||b.right>innerWidth+0.5))bad.push([e.className,Math.round(b.left),Math.round(b.right)])});const d=document.documentElement;return {bad,sw:d.scrollWidth,cw:d.clientWidth,mw:document.querySelector('#msgs').scrollWidth,mc:document.querySelector('#msgs').clientWidth}});return {ok:!r.bad.length&&r.sw<=r.cw&&r.mw<=r.mc,d:JSON.stringify(r)}});
const touch=async(p,type,pts)=>{p._c=p._c||await p.context().newCDPSession(p);await p._c.send('Input.dispatchTouchEvent',{type,touchPoints:pts})};
async function longpress(p,sel,{move=0,hold=750}={}){const b=await p.locator(sel).boundingBox();const x=b.x+b.width/2,y=b.y+b.height/2;
  await touch(p,'touchStart',[{x,y}]);if(move){for(let i=1;i<=4;i++){await touch(p,'touchMove',[{x,y:y+move*i/4}]);await sleep(p,30)}}await sleep(p,hold);await touch(p,'touchEnd',[])}
const MSG='.msg[data-mid="m1"]';
(async()=>{const b=await L.launch();
for(const dark of [false,true]){const th=dark?'dark':'light';
 // ---------- desktop ----------
 let p=await L.page(b,{w:1440,h:1000,dark});await p.goto(L.BASE+'groups.html');await sleep(p,400);
 if(!dark){
  await chk('desktop: Weekend hike selected by default',async()=>({ok:(await p.textContent('#convPane h2'))==='Weekend hike'}));
  await chk('desktop idle: no Reply/Discard text anywhere in the transcript',async()=>{const t=await idleText(p);return {ok:!/Reply|Discard/i.test(t),d:t.replace(/\s+/g,' ').slice(0,100)}});
  await chk('desktop idle: overflow controls hidden, no reserved action row (row height = bubble height)',async()=>{const r=await p.evaluate(()=>[...document.querySelectorAll('.brow')].map(r=>{const m=r.querySelector('.msg').getBoundingClientRect(),o=getComputedStyle(r.querySelector('.mo')).opacity;return [Math.round(r.getBoundingClientRect().height-m.height),o]}));return {ok:r.every(x=>x[0]<=1&&x[1]==='0'),d:JSON.stringify(r)}});
  await chk('desktop: no pre-existing .macts element',async()=>({ok:(await p.locator('.macts').count())===0}));
 }
 await shot(p,'desktop-'+th+'-idle');
 const w0=(await p.locator(MSG).boundingBox());
 await p.locator(MSG).hover();await sleep(p,200);
 await chk(th+' desktop hover reveals overflow beside bubble, bubble size unchanged',async()=>{const w1=await p.locator(MSG).boundingBox();const o=await p.locator('.brow[data-mrow=m1] .mo').boundingBox();const op=await p.$eval('.brow[data-mrow=m1] .mo',e=>getComputedStyle(e).opacity);return {ok:op==='1'&&Math.abs(w1.width-w0.width)<1&&Math.abs(w1.height-w0.height)<1&&o.x>=w1.x+w1.width-1,d:`op=${op} bubble ${w0.width}x${w0.height}->${w1.width}x${w1.height} mo.x=${o.x} bubbleRight=${w1.x+w1.width}`}});
 await shot(p,'desktop-'+th+'-hover');
 // keyboard
 await p.mouse.move(5,5);await p.focus('#msgs');await p.keyboard.press('Tab');
 await chk(th+' keyboard Tab reaches overflow control and reveals it',async()=>{const f=await foc(p);const op=await p.evaluate(()=>getComputedStyle(document.activeElement).opacity);return {ok:/Actions for/.test(f)&&op==='1',d:f+' op='+op}});
 const f1=await foc(p);await p.keyboard.press('Enter');await sleep(p,250);
 await chk(th+' Enter opens native dialog sheet; focus on Reply; has Reply+Cancel only for sent message',async()=>{const r=await p.evaluate(()=>{const d=document.querySelector('dialog[open]');return d?{f:document.activeElement.textContent.trim(),btns:[...d.querySelectorAll('button')].map(b=>b.textContent.trim()),lab:d.getAttribute('aria-labelledby')}:null});return {ok:!!r&&r.f==='Reply'&&r.btns.join()==='Reply,Cancel',d:JSON.stringify(r)}});
 await shot(p,'desktop-'+th+'-sheet');
 await p.keyboard.press('Escape');await sleep(p,250);
 await chk(th+' Escape closes sheet and restores focus to overflow control',async()=>{const f=await foc(p);return {ok:!(await dopen(p))&&f===f1,d:f+' vs '+f1}});
 await p.keyboard.press('Shift+F10');await sleep(p,250);
 await chk(th+' Shift+F10 opens sheet from focused control',async()=>({ok:await dopen(p)}));
 await p.click('dialog[open] button:has-text("Cancel")');await sleep(p,250);
 await chk(th+' Cancel restores focus to overflow control',async()=>{const f=await foc(p);return {ok:f===f1,d:f}});
 // reply flow
 await p.keyboard.press('Enter');await sleep(p,200);await p.keyboard.press('Enter');await sleep(p,300);
 await chk(th+' Reply: composer shows quote, textarea focused, no dialog',async()=>{const t=await p.textContent('.comp .reply').catch(()=>'');return {ok:/Sam/.test(t)&&(await foc(p))==='ta'&&!(await dopen(p)),d:t+' focus='+await foc(p)}});
 await shot(p,'desktop-'+th+'-replying');
 await p.keyboard.press('Escape');await sleep(p,200);
 await chk(th+' Escape cancels reply, textarea keeps focus',async()=>({ok:(await p.locator('.comp .reply').count())===0&&(await foc(p))==='ta'}));
 await p.locator('.msg[data-mid="m2"]').hover();await p.click('.brow[data-mrow=m2] .mo');await sleep(p,200);await p.click('dialog[open] button:has-text("Reply")');await sleep(p,200);
 await p.fill('#ta','Thanks, 7am works for me. I will bring a flask of tea for everyone and some spare socks as well.');await p.press('#ta','Enter');await sleep(p,1200);
 await chk(th+' send reply: quote inside bubble, composer reply bar gone, idle again',async()=>{const q=await p.locator('.rq').count();const r=await p.locator('.comp .reply').count();const t=await idleText(p);return {ok:q===1&&r===0&&!/Reply\b(?! to)|Discard/.test(t.replace(/Replying to/g,'')),d:`rq=${q} bar=${r}`}});
 await fit(p,th+' desktop');await shot(p,'desktop-'+th+'-sent');
 // queued/offline
 if(!dark){
  const d=p.locator('details',{hasText:'Simulation'}).last();await d.locator('summary').click();await p.locator('#simBody label:has-text("host desktops offline")').click();await sleep(p,400);
  await p.fill('#ta','nobody is hosting');await p.press('#ta','Enter');await sleep(p,500);
  await shot(p,'desktop-light-queued');
  await chk('queued message: dashed bubble keeps explicit waiting state; sheet offers Discard waiting message + Cancel',async()=>{const id=await p.$$eval('.msg.queued',e=>e.at(-1).dataset.mid);await p.locator('.msg[data-mid="'+id+'"]').hover();await p.click('.brow[data-mrow="'+id+'"] .mo');await sleep(p,200);const bt=await p.$$eval('dialog[open] button',e=>e.map(x=>x.textContent.trim().replace(/\s+/g,' ').slice(0,28)));return {ok:/Waiting for a host/.test(await idleText(p))&&bt.length===3&&/^Discard/.test(bt[1]),d:JSON.stringify(bt)}});
  await p.click('dialog[open] button:has-text("Discard")');await sleep(p,300);
  await chk('discard removes only the queued message, with undo toast; focus is not lost',async()=>{return {ok:(await p.locator('.msg.queued').count())===0&&/discarded/i.test(await p.textContent('#toastHost'))&&(await foc(p))!=='BODY',d:await foc(p)}});
 }
 await p.context().close();
 // ---------- phone ----------
 for(const [w,h] of [[390,844],[320,740]]){
  p=await L.page(b,{w,h,dark,mobile:true});await p.goto(L.BASE+'groups.html');await sleep(p,400);
  const tag=th+' '+w+'x'+h;
  await shot(p,w+'-'+th+'-list');
  await chk(tag+': opens on list (no deep link)',async()=>({ok:await p.locator('#app').getAttribute('data-view')==='list'}));
  await p.locator('[data-row=hike]').click();await sleep(p,400);
  await chk(tag+' idle: no Reply/Discard text; full-height conversation',async()=>{const t=await idleText(p);const a=await p.$eval('#app',e=>e.getBoundingClientRect().height);return {ok:!/Reply|Discard/i.test(t)&&Math.abs(a-h)<2,d:'app h='+a}});
  await chk(tag+': overflow target ≥44px, icon visible outside bubble',async()=>{const r=await p.evaluate(()=>[...document.querySelectorAll('.brow')].map(r=>{const o=r.querySelector('.mo').getBoundingClientRect(),m=r.querySelector('.msg').getBoundingClientRect(),op=getComputedStyle(r.querySelector('.mo')).opacity;const out=r.classList.contains('out');return [Math.round(o.width),Math.round(o.height),+op>0.5,out?o.right<=m.left+1:o.left>=m.right-1]}));return {ok:r.every(x=>x[0]>=44&&x[1]>=44&&x[2]&&x[3]),d:JSON.stringify(r)}});
  await fit(p,tag);await shot(p,w+'-'+th+'-conversation');
  // long-press shortcut
  await longpress(p,MSG);await sleep(p,250);
  await chk(tag+' long-press opens action sheet; sheet fits; focus in dialog',async()=>{const r=await p.evaluate(()=>{const d=document.querySelector('dialog[open]');if(!d)return null;const b=d.getBoundingClientRect();return {l:b.left,r:b.right,b:b.bottom,f:!!document.activeElement.closest('dialog')}});return {ok:!!r&&r.l>=0&&r.r<=w&&r.b<=h&&r.f,d:JSON.stringify(r)}});
  await shot(p,w+'-'+th+'-sheet');
  await p.keyboard.press('Escape');await sleep(p,250);
  await chk(tag+' Escape closes long-press sheet, focus restored to overflow control',async()=>{const f=await foc(p);return {ok:!(await dopen(p))&&/Actions for message from Sam/.test(f),d:f}});
  await longpress(p,MSG,{move:40});await sleep(p,200);
  await chk(tag+' long-press cancelled by finger movement',async()=>({ok:!(await dopen(p))}));
  await longpress(p,MSG,{hold:200});await sleep(p,500);
  await chk(tag+' short press (tap) does not open sheet',async()=>({ok:!(await dopen(p))}));
  // fill to scroll, then check scroll cancels + pointercancel
  for(let i=0;i<8;i++){await p.fill('#ta','Message number '+(i+1)+' to make the conversation scroll a little further.');await p.press('#ta','Enter');await sleep(p,60)}
  await sleep(p,900);
  await chk(tag+': transcript scrolls and composer stays visible',async()=>{const r=await p.evaluate(()=>{const m=document.querySelector('#msgs');return [m.scrollHeight>m.clientHeight,Math.round(document.querySelector('#ta').getBoundingClientRect().bottom)]});return {ok:r[0]&&r[1]<=h,d:JSON.stringify(r)}});
  await p.evaluate(()=>{document.querySelector('#msgs').scrollTop=0});await sleep(p,100);
  const bb=await p.locator('.msg[data-mid="m1"]').boundingBox();
  await touch(p,'touchStart',[{x:bb.x+20,y:bb.y+bb.height/2}]);await sleep(p,150);
  await p.evaluate(()=>{document.querySelector('#msgs').scrollTop=60});await sleep(p,700);await touch(p,'touchEnd',[]);
  await chk(tag+' long-press cancelled by scroll',async()=>({ok:!(await dopen(p))}));
  await p.evaluate(()=>{document.querySelector('#msgs').scrollTop=0});await sleep(p,100);
  const bc=await p.locator('.msg[data-mid="m1"]').boundingBox();
  await touch(p,'touchStart',[{x:bc.x+20,y:bc.y+bc.height/2}]);await sleep(p,150);await touch(p,'touchCancel',[]);await sleep(p,700);
  await chk(tag+' long-press cancelled by pointercancel',async()=>({ok:!(await dopen(p))}));
  // reply from sheet on phone
  await p.evaluate(()=>{document.querySelector('#msgs').scrollTop=0});await sleep(p,100);
  await p.click('.brow[data-mrow=m3] .mo');await sleep(p,250);await p.click('dialog[open] button:has-text("Reply")');await sleep(p,250);
  await chk(tag+' sheet Reply: composer quote visible, textarea focused; cancel button works',async()=>{const a=(await p.locator('.comp .reply').count())===1&&(await foc(p))==='ta';if(w===390)await shot(p,w+'-'+th+'-replying');await p.click('.comp .reply button');await sleep(p,150);return {ok:a&&(await p.locator('.comp .reply').count())===0,d:await foc(p)}});
  await fit(p,tag+' after sends');
  await p.fill('#ta','x');await p.focus('#ta');
  await chk(tag+': textarea selection unaffected by long-press handler (select all works)',async()=>{await p.fill('#ta','select me please');await p.focus('#ta');await p.keyboard.press('Control+A');return {ok:(await p.evaluate(()=>{const t=document.querySelector('#ta');return t.selectionEnd-t.selectionStart}))===16&&!(await dopen(p))}});
  await p.context().close();
 }
 // deep link
 p=await L.page(b,{w:390,h:844,dark,mobile:true});await p.goto(L.BASE+'groups.html?group=hike');await sleep(p,400);
 await chk(th+' deep link ?group=hike opens real mobile conversation',async()=>({ok:await p.locator('#app').getAttribute('data-view')==='conv'&&(await p.textContent('#convPane h2'))==='Weekend hike'}));
 if(!dark)await shot(p,'deeplink-390');
 await p.context().close();
}
// deep link validation
let p=await L.page(b,{w:390,h:844,mobile:true});
for(const bad of ['nope','%3Cscript%3E','hike%22%3E','../x']){await p.goto(L.BASE+'groups.html?group='+bad);await sleep(p,200);
 await chk('deep link invalid "'+bad+'" falls back to list',async()=>({ok:await p.locator('#app').getAttribute('data-view')==='list'&&p.errs.length===0,d:p.errs.join('|')}))}
await p.goto(L.BASE+'groups.html?group=book');await sleep(p,300);
await chk('deep link to removed group opens read-only conversation with no overflow controls',async()=>({ok:(await p.locator('.mo').count())===0&&(await p.locator('#ta').count())===0}));
await chk('no console errors / failed requests',async()=>({ok:p.errs.length===0&&p.fails.length===0,d:p.errs.concat(p.fails).join('|')}));
await p.context().close();
// regression: existing flow script is intact
await b.close();C.save('group-polish-results.json')})();
