const L=require('./lib'),C=require('./check');
const chk=C.mk('group-polish');const sleep=(p,ms)=>p.waitForTimeout(ms);
const foc=p=>p.evaluate(()=>{const a=document.activeElement;return a&&a!==document.body?(a.id||a.dataset.mid&&'bub:'+a.dataset.mid||a.getAttribute('aria-label')||(a.textContent||'').trim().slice(0,22)||a.tagName):'BODY'});
const dopen=p=>p.evaluate(()=>!!document.querySelector('dialog[open]'));
const items=p=>p.$$eval('dialog[open] .mi',e=>e.map(x=>x.textContent.trim()));
const shot=(p,n)=>p.screenshot({path:'screenshots/group-polish-'+n+'.png'});
const idleText=p=>p.evaluate(()=>document.querySelector('#msgs').innerText);
const simToggle=(p,txt)=>p.evaluate(t=>[...document.querySelectorAll('#simBody label')].find(l=>l.textContent.includes(t)).querySelector('input').click(),txt);
const fit=async(p,tag)=>chk(tag+': no clip/overflow',async()=>{const r=await p.evaluate(()=>{const m=document.querySelector('#msgs').getBoundingClientRect();const bad=[];document.querySelectorAll('#msgs .bub,.comp textarea,.comp .send').forEach(e=>{const b=e.getBoundingClientRect();if(b.width&&(b.left<m.left-0.5||b.right>m.right+0.5||b.right>innerWidth+0.5))bad.push([e.className,Math.round(b.left),Math.round(b.right)])});const d=document.documentElement;return {bad,sw:d.scrollWidth,cw:d.clientWidth,mw:document.querySelector('#msgs').scrollWidth,mc:document.querySelector('#msgs').clientWidth}});return {ok:!r.bad.length&&r.sw<=r.cw&&r.mw<=r.mc,d:JSON.stringify(r)}});
const touch=async(p,type,pts)=>{p._c=p._c||await p.context().newCDPSession(p);await p._c.send('Input.dispatchTouchEvent',{type,touchPoints:pts})};
async function longpress(p,sel,{move=0,hold=750}={}){const b=await p.locator(sel).boundingBox();const x=b.x+b.width/2,y=b.y+b.height/2;
  await touch(p,'touchStart',[{x,y}]);if(move){for(let i=1;i<=4;i++){await touch(p,'touchMove',[{x,y:y+move*i/4}]);await sleep(p,30)}}await sleep(p,hold);await touch(p,'touchEnd',[])}
const MSG='.bub[data-mid="m1"]';
(async()=>{const b=await L.launch();
for(const dark of [false,true]){const th=dark?'dark':'light';
 // ---------- desktop ----------
 let p=await L.page(b,{w:1440,h:1000,dark});await p.goto(L.BASE+'groups.html');await sleep(p,400);
 await shot(p,'desktop-'+th+'-idle');
 if(!dark){
  await chk('desktop: Weekend hike selected by default; header is one line "8 members"; no host strip',async()=>{const st=await p.textContent('#stLine');return {ok:(await p.textContent('#convPane h2'))==='Weekend hike'&&st==='8 members'&&(await p.locator('.strip').count())===0,d:st}});
  await chk('harness: slim bar, icon-only toggles, simulation drawer closed, no disclaimer visible',async()=>{const r=await p.evaluate(()=>({sim:document.querySelector('#sim').hidden,theme:document.querySelector('#theme').textContent.trim(),simb:document.querySelector('#simBtn').textContent.trim(),body:document.body.innerText}));return {ok:r.sim&&!r.theme&&!r.simb&&!/simulated|Nothing is sent|Dark mode/i.test(r.body),d:JSON.stringify({sim:r.sim,theme:r.theme})}});
  await chk('desktop idle: no Reply/Discard text, no visible message controls',async()=>{const t=await idleText(p);const vis=await p.$$eval('.mo',e=>e.filter(x=>getComputedStyle(x).opacity!=='0').length);return {ok:!/Reply|Discard/i.test(t)&&vis===0,d:'visible .mo='+vis}});
  await chk('grouping: corner classes and 2px/10px spacing for Sam’s run and own run',async()=>{const r=await p.evaluate(()=>['m1','m1b','m1c','m2','m3','m3b'].map(id=>{const e=document.querySelector('[data-mid='+id+']');return [id,[...e.classList].find(c=>c.startsWith('g-'))||'-',getComputedStyle(e.parentNode).marginTop]}));const exp='m1,g-first,m1b,g-mid,2px,m1c,g-last,2px,m2,-,10px,m3,g-first,10px,m3b,g-last,2px';const got=r.map(x=>x[0]==='m1'?[x[0],x[1]]:x).flat().join();return {ok:got===exp,d:got}});
  await chk('grouping: corners follow --r-bub / --r-bub-tight table',async()=>{const r=await p.evaluate(()=>['m1','m1b','m1c','m3','m3b'].map(id=>getComputedStyle(document.querySelector('[data-mid='+id+']')).borderRadius));return {ok:r.join('|')==='18px 18px 18px 6px|6px 18px 18px 6px|6px 18px 18px|18px 18px 6px|18px 6px 18px 18px',d:r.join('|')}});
  await chk('sender name only on first bubble of a group, 13px bold, avatar-hue ink',async()=>{const r=await p.evaluate(()=>{const w=document.querySelector('[data-mid=m1] .who'),cs=getComputedStyle(w);const n='Sam Rao',h='av'+([...n].reduce((a,c)=>a+c.codePointAt(0),0)%6);const probe=document.createElement('span');probe.style.color='var(--'+h+'-ink)';document.body.append(probe);const want=getComputedStyle(probe).color;probe.remove();return {b:!!document.querySelector('[data-mid=m1b] .who'),c:!!document.querySelector('[data-mid=m1c] .who'),fs:cs.fontSize,fw:cs.fontWeight,col:cs.color,want}});return {ok:!r.b&&!r.c&&r.fs==='13px'&&+r.fw>=700&&r.col===r.want,d:JSON.stringify(r)}});
  await chk('bubbles: sent mint no border, received surface + hairline border, canvas background',async()=>{const r=await p.evaluate(()=>{const v=n=>{const s=document.createElement('span');s.style.color='var('+n+')';document.body.append(s);const c=getComputedStyle(s).color;s.remove();return c};const o=getComputedStyle(document.querySelector('[data-mid=m3]')),i=getComputedStyle(document.querySelector('[data-mid=m2]'));return {ob:o.backgroundColor===v('--bub-out'),obw:o.borderTopWidth,ib:i.backgroundColor===v('--bub-in'),il:i.borderTopColor===v('--bub-in-line'),cv:getComputedStyle(document.querySelector('#msgs')).backgroundColor===v('--canvas')}});return {ok:r.ob&&r.obw==='0px'&&r.ib&&r.il&&r.cv,d:JSON.stringify(r)}});
  await chk('meta inline: single-line bubble stays one line tall; meta 11px tabular-nums inside bubble',async()=>{const r=await p.evaluate(()=>{const b=document.querySelector('[data-mid=m1c]'),m=b.querySelector('.meta'),bb=b.getBoundingClientRect(),mb=m.getBoundingClientRect(),cs=getComputedStyle(m);return {h:Math.round(bb.height),in:mb.bottom<=bb.bottom&&mb.right<=bb.right,fs:cs.fontSize,num:cs.fontVariantNumeric,fl:cs.float}});return {ok:r.h<=36&&r.in&&r.fs==='11px'&&/tabular/.test(r.num)&&r.fl==='right',d:JSON.stringify(r)}});
  await chk('composer: attach 44 · field ≥44 · paper-plane send 44 at 38% when empty',async()=>{const r=await p.evaluate(()=>{const s=document.querySelector('#sendBtn'),t=document.querySelector('#ta'),a=document.querySelector('.att');return {s:s.offsetWidth,sh:s.offsetHeight,dis:s.disabled,op:getComputedStyle(s).opacity,plane:s.innerHTML.includes('M22 2L11 13'),t:t.offsetHeight,a:a.offsetWidth,ph:t.placeholder,hint:document.querySelector('.comp').innerText.trim()}});return {ok:r.s===44&&r.sh===44&&r.dis&&r.op==='0.38'&&r.plane&&r.t>=44&&r.a===44&&r.ph==='Message the group'&&r.hint==='',d:JSON.stringify(r)}});
  await p.fill('#ta','x');
  await chk('composer: send enabled when draft present',async()=>({ok:await p.evaluate(()=>!document.querySelector('#sendBtn').disabled&&getComputedStyle(document.querySelector('#sendBtn')).opacity==='1')}));
  await p.fill('#ta','');
  await chk('sidebar: request rows are quiet rows with t-badge counts, no banner',async()=>{const r=await p.evaluate(()=>({rows:[...document.querySelectorAll('.qrow')].map(x=>x.querySelector('.nm').textContent+':'+x.querySelector('.t-badge').firstChild.textContent),wait:document.querySelectorAll('.wait,details').length,txt:/Waiting for you/.test(document.querySelector('#listPane').innerText)}));return {ok:r.rows.join()==='Invitations:2,Chat requests:1,Sent requests:2'&&!r.wait&&!r.txt,d:JSON.stringify(r)}});
  await chk('event line centered 12px muted; day separator is a pill',async()=>{const r=await p.evaluate(()=>{const s=getComputedStyle(document.querySelector('.sys')),d=getComputedStyle(document.querySelector('.day'));return [s.fontSize,s.textAlign,d.fontSize,d.borderRadius,document.querySelector('.day').textContent]});return {ok:r[0]==='12px'&&r[1]==='center'&&r[2]==='11px'&&parseFloat(r[3])>=99&&r[4]==='Today',d:r.join()}});
 }
 const w0=await p.locator(MSG).boundingBox();
 await p.locator(MSG).hover();await sleep(p,250);
 await chk(th+' desktop hover reveals 24px chevron beside bubble, bubble size unchanged',async()=>{const w1=await p.locator(MSG).boundingBox();const o=await p.locator('.brow[data-mrow=m1] .mo').boundingBox();const op=await p.$eval('.brow[data-mrow=m1] .mo',e=>getComputedStyle(e).opacity);return {ok:op==='1'&&Math.abs(w1.width-w0.width)<1&&Math.abs(w1.height-w0.height)<1&&o.x>=w1.x+w1.width-1&&o.width===24,d:`op=${op} mo.x=${o.x} w=${o.width} bubbleRight=${w1.x+w1.width}`}});
 await shot(p,'desktop-'+th+'-hover');
 await p.click('.brow[data-mrow=m1] .mo');await sleep(p,250);
 await chk(th+' chevron opens anchored popover (not a sheet): Reply, Copy, Message info',async()=>{const it=await items(p);const r=await p.evaluate(()=>{const d=document.querySelector('dialog[open]'),m=document.querySelector('.brow[data-mrow=m1] .mo').getBoundingClientRect(),b=d.getBoundingClientRect();return {pop:d.classList.contains('dpop'),dy:Math.round(b.top-m.bottom),w:Math.round(b.width)}});return {ok:it.join()==='Reply,Copy,Message info'&&r.pop&&r.dy>=0&&r.dy<=12&&r.w<300,d:it.join()+' '+JSON.stringify(r)}});
 await shot(p,'desktop-'+th+'-popover');
 await p.keyboard.press('Escape');await sleep(p,250);
 await chk(th+' Escape closes popover, focus returns to the message’s menu button',async()=>{const f=await foc(p);return {ok:!(await dopen(p))&&/Actions for message from Sam/.test(f),d:f}});
 // keyboard path
 await p.mouse.move(5,5);await p.focus('#msgs');await p.keyboard.press('Tab');
 await chk(th+' keyboard Tab reaches the menu button and reveals it',async()=>{const f=await foc(p);const op=await p.evaluate(()=>getComputedStyle(document.activeElement).opacity);return {ok:/Actions for/.test(f)&&op==='1',d:f+' op='+op}});
 const f1=await foc(p);await p.keyboard.press('Enter');await sleep(p,250);
 await chk(th+' Enter opens the menu with focus on Reply',async()=>{const f=await p.evaluate(()=>document.activeElement.textContent.trim());return {ok:(await dopen(p))&&f==='Reply',d:f}});
 await p.keyboard.press('Escape');await sleep(p,250);
 await chk(th+' Escape restores focus to the menu button',async()=>{const f=await foc(p);return {ok:!(await dopen(p))&&f===f1,d:f}});
 // own message menu + message info
 await p.locator('[data-mid=m3]').hover();await p.click('.brow[data-mrow=m3] .mo');await sleep(p,250);
 await chk(th+' own message menu: Reply, Copy, Edit, Delete, Message info',async()=>{const it=await items(p);return {ok:it.join()==='Reply,Copy,Edit,Delete,Message info',d:it.join()}});
 await p.click('dialog[open] .mi:has-text("Message info")');await sleep(p,300);
 await chk(th+' Message info explains delivery (not in the bubble)',async()=>{const t=await p.textContent('dialog[open]');return {ok:/Delivered to the group’s hosts/.test(t)&&!/Delivered to/.test(await idleText(p)),d:t.replace(/\s+/g,' ').slice(0,90)}});
 await p.keyboard.press('Escape');await sleep(p,250);
 await chk(th+' closing Message info returns focus to the message’s menu button',async()=>{const f=await foc(p);return {ok:/Actions for your message/.test(f),d:f}});
 // reply flow
 await p.locator('[data-mid=m1]').hover();await p.click('.brow[data-mrow=m1] .mo');await sleep(p,200);await p.click('dialog[open] .mi:has-text("Reply")');await sleep(p,300);
 await chk(th+' Reply: context strip names Sam, textarea focused, no dialog',async()=>{const t=await p.textContent('.comp .reply').catch(()=>'');return {ok:/Sam/.test(t)&&(await foc(p))==='ta'&&!(await dopen(p)),d:t+' focus='+await foc(p)}});
 await shot(p,'desktop-'+th+'-replying');
 await p.keyboard.press('Escape');await sleep(p,200);
 await chk(th+' Escape cancels reply, textarea keeps focus',async()=>({ok:(await p.locator('.comp .reply').count())===0&&(await foc(p))==='ta'}));
 await p.locator('[data-mid=m2]').hover();await p.click('.brow[data-mrow=m2] .mo');await sleep(p,200);await p.click('dialog[open] .mi:has-text("Reply")');await sleep(p,200);
 await p.fill('#ta','Thanks, 7am works for me. I will bring a flask of tea for everyone and some spare socks as well.');await p.press('#ta','Enter');await sleep(p,1200);
 await chk(th+' send reply: quote inside bubble, strip gone, delivered tick',async()=>{const q=await p.locator('.bub .rq').count();const r=await p.locator('.comp .reply').count();const st=await p.$$eval('.bub.out',e=>e.at(-1).dataset.state);return {ok:q===1&&r===0&&st==='delivered',d:`rq=${q} bar=${r} state=${st}`}});
 // edit
 const last=await p.$$eval('.bub.out',e=>e.at(-1).dataset.mid);
 await p.locator('[data-mid="'+last+'"]').hover();await p.click('.brow[data-mrow="'+last+'"] .mo');await sleep(p,200);await p.click('dialog[open] .mi:has-text("Edit")');await sleep(p,250);
 await chk(th+' Edit puts the text in the composer with an Editing strip',async()=>({ok:/Editing/.test(await p.textContent('.comp .reply'))&&/flask of tea/.test(await p.inputValue('#ta'))}));
 await p.fill('#ta','Thanks, 7am works. Bringing tea.');await p.press('#ta','Enter');await sleep(p,300);
 await chk(th+' edited message shows "edited" in the inline meta',async()=>{const t=await p.textContent('[data-mid="'+last+'"] .meta');return {ok:/^edited \d\d:\d\d/.test(t),d:t}});
 await fit(p,th+' desktop');await shot(p,'desktop-'+th+'-sent');
 // right click
 await p.locator('[data-mid=m4]').click({button:'right'});await sleep(p,250);
 await chk(th+' right-click opens the same popover',async()=>({ok:await p.evaluate(()=>!!document.querySelector('dialog.dpop[open]'))}));
 await p.keyboard.press('Escape');await sleep(p,200);
 // offline host
 await p.mouse.move(5,5);
 await simToggle(p,'host desktops offline');await sleep(p,400);
 await chk(th+' offline host: thin warn strip under header, title block unchanged',async()=>{const t=await p.textContent('.strip').catch(()=>'');return {ok:/^Host offline — messages wait/.test(t)&&(await p.textContent('#stLine'))==='8 members',d:t}});
 await p.fill('#ta','nobody is hosting');await p.press('#ta','Enter');await sleep(p,500);
 await shot(p,'desktop-'+th+'-offline');
 await chk(th+' queued message: pending clock glyph, no explanatory text in bubble',async()=>{const r=await p.$$eval('.bub.out',e=>{const b=e.at(-1),c=b.cloneNode(true);c.querySelectorAll('.sr').forEach(x=>x.remove());return {st:b.dataset.state,clock:!!b.querySelector('.tick.pend'),txt:c.textContent}});return {ok:r.st==='queued'&&r.clock&&!/Waiting|saved/i.test(r.txt),d:JSON.stringify(r)}});
 if(!dark){
  const id=await p.$$eval('.bub[data-state=queued]',e=>e.at(-1).dataset.mid);
  await p.locator('[data-mid="'+id+'"]').hover();await p.click('.brow[data-mrow="'+id+'"] .mo');await sleep(p,200);await p.click('dialog[open] .mi:has-text("Delete")');await sleep(p,300);
  await chk('delete removes only the queued message, with undo toast; focus not lost',async()=>({ok:(await p.locator('.bub[data-state=queued]').count())===0&&/deleted/i.test(await p.textContent('#toastHost'))&&(await foc(p))!=='BODY',d:await foc(p)}));
  await simToggle(p,'host desktops offline');await sleep(p,300);
  await chk('host back: strip disappears',async()=>({ok:(await p.locator('.strip').count())===0}));
  // incoming while scrolled up
  for(let i=0;i<6;i++){await p.fill('#ta','Filler message '+i+' so that the conversation needs to scroll on a large desktop window, quite long text here.');await p.press('#ta','Enter');await sleep(p,40)}
  await sleep(p,800);await p.evaluate(()=>{document.querySelector('#msgs').scrollTop=0});await sleep(p,100);
  await p.evaluate(()=>[...document.querySelectorAll('#simBody button')].find(b=>/Sam sends/.test(b.textContent)).click());await sleep(p,300);
  await chk('incoming while scrolled up: 36px scroll-to-latest button with t-badge',async()=>{const r=await p.evaluate(()=>{const t=document.querySelector('#tobot');return t&&{w:t.offsetWidth,b:t.querySelector('.t-badge').textContent}});return {ok:!!r&&r.w===36&&r.b==='1',d:JSON.stringify(r)}});
  await p.click('#tobot');await sleep(p,200);
  await chk('scroll-to-latest clears the badge',async()=>({ok:(await p.locator('#tobot').count())===0}));
 }
 // new group dialog
 await p.click('#newBtn');await sleep(p,300);await p.fill('#gn','Climbing crew');await p.locator('dialog[open] input[type=checkbox]').nth(0).check();await sleep(p,100);
 await shot(p,'desktop-'+th+'-newgroup');
 await p.keyboard.press('Escape');await sleep(p,200);
 await p.context().close();
 // ---------- phone ----------
 for(const [w,h] of [[390,844],[320,740]]){
  p=await L.page(b,{w,h,dark,mobile:true});await p.goto(L.BASE+'groups.html');await sleep(p,400);
  const tag=th+' '+w+'x'+h;
  await shot(p,w+'-'+th+'-list');
  await chk(tag+': opens on list; New group is a 56px FAB',async()=>{const r=await p.$eval('#newBtn',e=>[e.offsetWidth,getComputedStyle(e).position]);return {ok:await p.locator('#app').getAttribute('data-view')==='list'&&r[0]===56&&r[1]==='absolute',d:r.join()}});
  await chk(tag+': no list-row selection tint on mobile',async()=>({ok:await p.evaluate(()=>[...document.querySelectorAll('.row')].every(r=>getComputedStyle(r).backgroundColor==='rgba(0, 0, 0, 0)'))}));
  await p.locator('[data-row=hike]').click();await sleep(p,400);
  await chk(tag+' idle: no Reply text; full-height conversation; no visible menu controls',async()=>{const t=await idleText(p);const a=await p.$eval('#app',e=>e.getBoundingClientRect().height);const vis=await p.$$eval('.mo',e=>e.filter(x=>x.getBoundingClientRect().width>1).length);return {ok:!/Reply|Discard/i.test(t)&&Math.abs(a-h)<2&&vis===0,d:'app h='+a+' visible mo='+vis}});
  await fit(p,tag);await shot(p,w+'-'+th+'-conversation');
  await longpress(p,MSG);await sleep(p,300);
  await chk(tag+' long-press opens bottom sheet anchored to the bottom; focus inside',async()=>{const r=await p.evaluate(()=>{const d=document.querySelector('dialog[open]');if(!d)return null;const b=d.getBoundingClientRect();return {menu:d.classList.contains('dmenu'),l:b.left,r:b.right,b:Math.round(b.bottom),f:!!document.activeElement.closest('dialog')}});return {ok:!!r&&r.menu&&r.l>=0&&r.r<=w&&Math.abs(r.b-h)<2&&r.f,d:JSON.stringify(r)}});
  await chk(tag+' sheet actions for received message',async()=>{const it=await items(p);return {ok:it.join()==='Reply,Copy,Message info',d:it.join()}});
  await shot(p,w+'-'+th+'-sheet');
  await p.keyboard.press('Escape');await sleep(p,250);
  await chk(tag+' Escape closes sheet; focus returns to the message',async()=>{const f=await foc(p);return {ok:!(await dopen(p))&&f==='bub:m1',d:f}});
  await longpress(p,MSG,{move:40});await sleep(p,200);
  await chk(tag+' long-press cancelled by finger movement',async()=>({ok:!(await dopen(p))}));
  await longpress(p,MSG,{hold:200});await sleep(p,500);
  await chk(tag+' short press (tap) does not open sheet',async()=>({ok:!(await dopen(p))}));
  await p.fill('#ta','line one');await p.press('#ta','Enter');
  await chk(tag+': Enter on touch inserts a newline (does not send)',async()=>({ok:(await p.inputValue('#ta'))==='line one\n'}));
  for(let i=0;i<8;i++){await p.fill('#ta','Message number '+(i+1)+' to make the conversation scroll a little further.');await p.click('#sendBtn');await sleep(p,60)}
  await sleep(p,900);
  await chk(tag+': transcript scrolls and composer stays visible',async()=>{const r=await p.evaluate(()=>{const m=document.querySelector('#msgs');return [m.scrollHeight>m.clientHeight,Math.round(document.querySelector('#ta').getBoundingClientRect().bottom)]});return {ok:r[0]&&r[1]<=h,d:JSON.stringify(r)}});
  await p.evaluate(()=>{document.querySelector('#msgs').scrollTop=0});await sleep(p,100);
  const bb=await p.locator(MSG).boundingBox();
  await touch(p,'touchStart',[{x:bb.x+20,y:bb.y+bb.height/2}]);await sleep(p,150);
  await p.evaluate(()=>{document.querySelector('#msgs').scrollTop=60});await sleep(p,700);await touch(p,'touchEnd',[]);
  await chk(tag+' long-press cancelled by scroll',async()=>({ok:!(await dopen(p))}));
  await p.evaluate(()=>{document.querySelector('#msgs').scrollTop=0});await sleep(p,100);
  const bc=await p.locator(MSG).boundingBox();
  await touch(p,'touchStart',[{x:bc.x+20,y:bc.y+bc.height/2}]);await sleep(p,150);await touch(p,'touchCancel',[]);await sleep(p,700);
  await chk(tag+' long-press cancelled by pointercancel',async()=>({ok:!(await dopen(p))}));
  await p.evaluate(()=>{document.querySelector('#msgs').scrollTop=0});await sleep(p,100);
  await longpress(p,'.bub[data-mid=m3]');await sleep(p,300);
  await chk(tag+' own message sheet includes Edit and Delete',async()=>{const it=await items(p);return {ok:it.join()==='Reply,Copy,Edit,Delete,Message info',d:it.join()}});
  await p.click('dialog[open] .mi:has-text("Reply")');await sleep(p,250);
  await chk(tag+' sheet Reply: strip visible, textarea focused; cancel works',async()=>{const a=(await p.locator('.comp .reply').count())===1&&(await foc(p))==='ta';if(w===390)await shot(p,w+'-'+th+'-replying');await p.click('.comp .reply button');await sleep(p,150);return {ok:a&&(await p.locator('.comp .reply').count())===0,d:await foc(p)}});
  await fit(p,tag+' after sends');
  await chk(tag+': textarea select-all unaffected by long-press handler',async()=>{await p.fill('#ta','select me please');await p.focus('#ta');await p.keyboard.press('Control+A');return {ok:(await p.evaluate(()=>{const t=document.querySelector('#ta');return t.selectionEnd-t.selectionStart}))===16&&!(await dopen(p))}});
  if(w===390){
   await p.fill('#ta','');await simToggle(p,'host desktops offline');await sleep(p,400);
   await p.fill('#ta','Waiting on a host');await p.click('#sendBtn');await sleep(p,400);
   await chk(tag+' offline strip under header on phone',async()=>({ok:/Host offline — messages wait/.test(await p.textContent('.strip').catch(()=>''))}));
   await shot(p,w+'-'+th+'-offline');
   await p.click('[aria-label="Back to groups"]');await sleep(p,300);await p.click('#newBtn');await sleep(p,300);
   await shot(p,w+'-'+th+'-newgroup');
   await chk(tag+' new-group sheet sits at the bottom and fits',async()=>{const r=await p.evaluate(()=>{const b=document.querySelector('dialog[open]').getBoundingClientRect();return [b.left,b.right,Math.round(b.bottom),b.top]});return {ok:r[0]>=0&&r[1]<=w&&Math.abs(r[2]-h)<2&&r[3]>=0,d:JSON.stringify(r)}});
  }
  await p.context().close();
 }
 // deep link
 p=await L.page(b,{w:390,h:844,dark,mobile:true});await p.goto(L.BASE+'groups.html?group=hike');await sleep(p,400);
 await chk(th+' deep link ?group=hike opens real mobile conversation',async()=>({ok:await p.locator('#app').getAttribute('data-view')==='conv'&&(await p.textContent('#convPane h2'))==='Weekend hike'}));
 await p.context().close();
}
// embed contract
let p=await L.page(b,{w:390,h:844,mobile:true});
await p.goto(L.BASE+'groups.html?group=hike&preview=mobile');await sleep(p,400);
await chk('mobile.html contract (?group=hike&preview=mobile): conversation, harness hidden',async()=>({ok:await p.locator('#app').getAttribute('data-view')==='conv'&&!(await p.locator('#hbar').isVisible())}));
await p.screenshot({path:'screenshots/group-polish-embed-mobile.png'});
await p.context().close();
p=await L.page(b,{w:1000,h:700});await p.goto(L.BASE+'groups.html?embed=1&group=hike');await sleep(p,400);
await chk('?embed=1: no harness/drawer, zero padding, frame fills 100dvh without border/radius',async()=>{const r=await p.evaluate(()=>{const a=document.querySelector('#app'),cs=getComputedStyle(a),b=a.getBoundingClientRect();return {bar:getComputedStyle(document.querySelector('#hbar')).display,sim:getComputedStyle(document.querySelector('#sim')).display,top:b.top,left:b.left,h:Math.round(b.height),bw:cs.borderTopWidth,rad:cs.borderTopLeftRadius,list:getComputedStyle(document.querySelector('#listPane')).display}});return {ok:r.bar==='none'&&r.sim==='none'&&r.top===0&&r.left===0&&r.h===700&&r.bw==='0px'&&r.rad==='0px'&&r.list==='none',d:JSON.stringify(r)}});
await p.screenshot({path:'screenshots/group-polish-embed-desktop.png'});
await chk('?embed=1&group: conversation locked — no back control, no list',async()=>({ok:!(await p.locator('.backbtn').isVisible())&&!(await p.locator('#listPane').isVisible())}));
await p.setViewportSize({width:620,height:700});await p.goto(L.BASE+'groups.html?group=family&embed=1');await sleep(p,300);
await chk('?embed=1 at narrow width: still the locked conversation, no back control',async()=>({ok:await p.locator('#app').getAttribute('data-view')==='conv'&&(await p.textContent('#convPane h2'))==='Family'&&!(await p.locator('.backbtn').isVisible())}));
await p.goto(L.BASE+'groups.html?group=book&embed=1&theme=dark&motion=reduce');await sleep(p,300);
await chk('theme/motion query params honoured; home-style attribute sync also works',async()=>{const a=await p.evaluate(()=>[document.documentElement.dataset.theme,document.documentElement.dataset.motion,getComputedStyle(document.querySelector('#msgs')).backgroundColor]);await p.evaluate(()=>{document.documentElement.dataset.theme='light'});const b=await p.evaluate(()=>getComputedStyle(document.querySelector('#msgs')).backgroundColor);return {ok:a[0]==='dark'&&a[1]==='reduce'&&a[2]!==b,d:a.join()+' -> '+b}});
await p.setViewportSize({width:1000,height:700});
await p.goto(L.BASE+'groups.html?embed=1');await sleep(p,300);
await chk('?embed=1 without a group keeps the two-pane app',async()=>({ok:await p.evaluate(()=>getComputedStyle(document.querySelector('#listPane')).display!=='none'&&getComputedStyle(document.querySelector('#hbar')).display==='none')}));
await p.context().close();
// deep link validation
p=await L.page(b,{w:390,h:844,mobile:true});
for(const bad of ['nope','%3Cscript%3E','hike%22%3E','../x']){await p.goto(L.BASE+'groups.html?group='+bad);await sleep(p,200);
 await chk('deep link invalid "'+bad+'" falls back to list',async()=>({ok:await p.locator('#app').getAttribute('data-view')==='list'&&p.errs.length===0,d:p.errs.join('|')}))}
for(const id of ['hike','family','book']){await p.goto(L.BASE+'groups.html?group='+id+'&from=home');await sleep(p,250);
 await chk('mobile ?group='+id+'&from=home deep-links into the conversation; back goes home',async()=>({ok:await p.locator('#app').getAttribute('data-view')==='conv'&&(await p.locator('[aria-label="Back to home"]').count())===1&&p.errs.length===0,d:p.errs.join('|')}))}
await p.goto(L.BASE+'groups.html?group=book');await sleep(p,300);
await chk('deep link to removed group: read-only, no menu buttons, no composer',async()=>({ok:(await p.locator('.mo').count())===0&&(await p.locator('#ta').count())===0}));
await chk('no console errors / failed requests',async()=>({ok:p.errs.length===0&&p.fails.length===0,d:p.errs.concat(p.fails).join('|')}));
await p.context().close();
await b.close();C.save('group-polish-results.json')})();
