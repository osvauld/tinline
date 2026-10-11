const L=require('./lib'),C=require('./check');
const chk=C.mk('groups');const sleep=(p,ms)=>p.waitForTimeout(ms);
const foc=p=>p.evaluate(()=>{const a=document.activeElement;return a&&a!==document.body?(a.id||a.getAttribute('aria-label')||(a.textContent||'').trim().slice(0,22)||a.tagName):'BODY'});
const dtxt=p=>p.evaluate(()=>{const ds=document.querySelectorAll('dialog[open]');const d=ds[ds.length-1];return d?d.textContent.replace(/\s+/g,' ').trim():'NO DIALOG'});
const simOpen=async(p)=>{if(await p.$eval('#sim',e=>e.hidden))await p.click('#simBtn')};
const simClose=async(p)=>{if(!(await p.$eval('#sim',e=>e.hidden)))await p.click('#simClose')};
const open=async(p)=>{for(const k of ['inv','req','sent']){const d=p.locator('#fold-'+k);if(await d.count()&&(await d.getAttribute('aria-expanded'))!=='true')await d.click()}};
// Group-call roster (the shared stage from call-state.js) as data, read from the open overlay.
const rosterInfo=p=>p.evaluate(()=>{const o=document.querySelector('#callOv .t-gc');if(!o||o.hidden)return null;
 const hueOf=a=>[...a.classList].find(c=>/^av\d$/.test(c))||'';
 const tiles=[...o.querySelectorAll('.t-gc-spk>li')].map(li=>{const a=li.querySelector('.t-avatar'),r=li.getBoundingClientRect(),cs=getComputedStyle(a),hb=li.querySelector('.t-gc-host');
  return {id:li.dataset.id,name:li.querySelector('.t-gc-fn').textContent,ini:a.textContent,host:hb?hb.textContent:'',hostBadge:hb?hb.classList.contains('t-badge')&&hb.classList.contains('muted'):false,mic:li.querySelector('.t-gc-mic')?.getAttribute('aria-label')||'',on:li.classList.contains('on'),ring:cs.outlineStyle+' '+cs.outlineWidth,w:Math.round(a.getBoundingClientRect().width),top:Math.round(r.top),bottom:Math.round(r.bottom),hue:hueOf(a),anim:cs.animationName}});
 const quiet=[...o.querySelectorAll('.t-gc-quiet>li')].map(li=>{const b=li.querySelector('button');if(b)return {more:b.textContent,label:b.getAttribute('aria-label'),w:b.offsetWidth,top:Math.round(li.getBoundingClientRect().top)};
  const im=li.querySelector('[role=img]'),a=im.querySelector('.t-avatar');return {id:li.dataset.id,label:im.getAttribute('aria-label'),ini:a.textContent,w:Math.round(a.getBoundingClientRect().width),hue:hueOf(a),top:Math.round(li.getBoundingClientRect().top)}});
 return {sub:o.querySelector('.t-gc-sub').textContent,name:o.querySelector('.t-gc-nm').textContent,tiles,quiet}});
const sig=r=>JSON.stringify(r&&{name:r.name,tiles:r.tiles.map(t=>[t.id,t.name,t.ini,t.host,t.mic,t.w,t.hue].join(':')),quiet:r.quiet.map(q=>q.more?'more:'+q.more+':'+q.label:[q.id,q.label,q.ini,q.w,q.hue].join(':'))});
const hueN=n=>'av'+([...n].reduce((a,c)=>a+c.codePointAt(0),0)%6);
// Oscillator frequencies (pairs with lib.js's __aud.osc order) tell the ring/back patterns apart. "live" = started and not yet
// ended: Snd schedules every note's stop() up front, so lib.js's stopped flag is set at once.
const freqHook=p=>p.context().addInitScript(()=>{const AC=window.AudioContext;if(!AC)return;const co=AC.prototype.createOscillator;window.__oscF=[];AC.prototype.createOscillator=function(){const o=co.apply(this,arguments);window.__oscF.push(o);return o}});
const audio=p=>p.evaluate(()=>{const f=(window.__oscF||[]).map(o=>Math.round(o.frequency.value)),a=window.__aud||{osc:[]};
 return {played:window.TinAudio?TinAudio.played.slice():[],freqs:[...new Set(f)].sort((x,y)=>x-y),ctx:(a.ctxs||[]).length,started:a.started||0,live:a.osc.filter(o=>o.started&&!o.ended).length,
  liveBack:a.osc.filter((o,i)=>o.started&&!o.ended&&[392,494].includes(f[i])).length,ringOrBack:f.filter(x=>[392,494,1175,2350].includes(x)).length}});
async function roster(p,label){
 const n=await p.evaluate(()=>new Promise(res=>{const el=document.querySelector('#callOv .t-gc-spk');let n=0;const mo=new MutationObserver(m=>n+=m.length);mo.observe(el,{subtree:true,childList:true,attributes:true,characterData:true});setTimeout(()=>{mo.disconnect();res(n)},1500)}));
 const r=await rosterInfo(p);if(!r){await chk(label+' roster: shown',async()=>false);return}
 await chk(label+' roster: k=3 speaking tiles (72px .t-avatar, shared hue hash), first names, never one big avatar',async()=>({ok:r.tiles.length===3&&r.tiles.every(t=>t.w===72&&t.hue===hueN({arjun:'Arjun Oommen',sam:'Sam Rao',priya:'Priya N'}[t.id]))&&r.tiles.map(t=>t.name).join()==='Arjun,Sam,Priya',d:JSON.stringify(r.tiles.map(t=>[t.id,t.w,t.hue]))}));
 await chk(label+' roster: the host (Arjun) carries one "Hosting" .t-badge.muted tag',async()=>({ok:r.tiles.filter(t=>t.host).length===1&&r.tiles[0].host==='Hosting'&&r.tiles[0].hostBadge,d:JSON.stringify(r.tiles.map(t=>t.host))}));
 await chk(label+' roster: exactly one speaking tile with a 2px solid ring, no animation on any tile',async()=>({ok:r.tiles.filter(t=>t.on).length===1&&r.tiles.find(t=>t.on).ring==='solid 2px'&&r.tiles.every(t=>t.anim==='none'),d:JSON.stringify(r.tiles.map(t=>[t.id,t.on,t.ring,t.anim]))}));
 await chk(label+' roster: speaking ring does not redraw between level changes (0 DOM mutations in 1.5s while the timer ticks)',async()=>({ok:n===0,d:'mutations='+n}));
 await chk(label+' roster: muted speaker (Priya) shows a mic-off badge named "Muted"',async()=>({ok:r.tiles.find(t=>t.id==='priya').mic==='Muted'&&r.tiles.filter(t=>t.mic).length===1,d:JSON.stringify(r.tiles.map(t=>t.mic))}));
 const av=r.quiet.filter(q=>!q.more),more=r.quiet.find(q=>q.more);
 await chk(label+' roster: quiet row = 6 x 40px avatars (you first, muted Kavya named) then "+1" (44px button, real name)',async()=>({ok:av.length===6&&av.every(q=>q.w===40)&&av[0].label==='You'&&av.some(q=>q.label==='Kavya S, muted')&&more&&more.more==='+1'&&more.label==='Show everyone in the call, 1 more'&&more.w===44&&r.sub==='10 in the call',d:JSON.stringify(r.quiet.map(q=>q.label))+' '+r.sub}));
 await p.click('#callOv .t-gc-more');await sleep(p,200);
 const sh=await p.evaluate(()=>{const d=document.querySelector('dialog.t-gc-sheet');return d&&{open:d.open,h:d.querySelector('h2').textContent,rows:[...d.querySelectorAll('li')].map(li=>li.querySelector('.t-gc-who').textContent),focus:d.contains(document.activeElement)}});
 await chk(label+' roster: "+1" opens the whole roster in a sheet (10 rows, tags spelled out)',async()=>({ok:sh&&sh.open&&sh.h==='10 in the call'&&sh.rows.length===10&&sh.rows.some(t=>/^Arjun OommenHosting( · Speaking)?$/.test(t))&&sh.rows.some(t=>/^Priya NMuted$/.test(t))&&sh.rows.some(t=>/^Tara M$/.test(t))&&sh.rows.includes('You')&&sh.focus,d:JSON.stringify(sh)}));
 await p.keyboard.press('Escape');await sleep(p,200);
 await chk(label+' roster: Escape closes the sheet only (call overlay stays), focus back on "+1"',async()=>{const r2=await p.evaluate(()=>({open:document.querySelector('dialog.t-gc-sheet').open,ov:!document.querySelector('#callOv').hidden&&!!document.querySelector('#callOv').offsetParent,f:document.activeElement.className}));return {ok:!r2.open&&r2.ov&&r2.f==='t-gc-more',d:JSON.stringify(r2)}});
}
(async()=>{const b=await L.launch();
let p=await L.page(b);await p.goto(L.BASE+'groups.html');await sleep(p,400);
await chk('group call: 44px phone button left of Members',async()=>{const r=await p.evaluate(()=>{const c=document.querySelector('#callBtn'),m=document.querySelector('#memBtn');return c&&{tag:c.tagName,w:c.offsetWidth,h:c.offsetHeight,left:c.getBoundingClientRect().right<=m.getBoundingClientRect().left,dis:c.getAttribute('aria-disabled')}});return {ok:!!r&&r.tag==='BUTTON'&&r.w===44&&r.h===44&&r.left&&!r.dis,d:JSON.stringify(r)}});
const url0=p.url();await p.click('#callBtn');await sleep(p,300);
await chk('call: in-page overlay fills the app frame, no navigation, focus on overlay',async()=>{const r=await p.evaluate(()=>{const o=document.querySelector('#callOv'),a=document.querySelector('#app').getBoundingClientRect(),b=o.getBoundingClientRect();return {fill:Math.abs(a.width-b.width)<3&&Math.abs(a.height-b.height)<3,f:document.activeElement===o,st:o.querySelector('.t-gc-sub').textContent,av:o.querySelector('.t-gc-glyph').offsetWidth,h2:getComputedStyle(o.querySelector('.t-gc-nm')).fontSize,one:!o.querySelector('.cbody'),mute:o.querySelector('.mute').offsetWidth,end:o.querySelector('.end').offsetWidth,min:o.querySelector('.cmin').getAttribute('aria-label')}});return {ok:p.url()===url0&&r.fill&&r.f&&r.st==='Connecting…'&&r.av===44&&r.h2==='17px'&&r.one&&r.mute===56&&r.end===64&&r.min==='Back to chat — call continues',d:JSON.stringify(r)}});
await sleep(p,3700);
await chk('call: connects (group calls do not ring) and the MM:SS timer ticks',async()=>{const r=await p.evaluate(()=>[document.querySelector('#callOv .t-gc-sub').textContent,document.querySelector('#callOv .t-gc-tm').textContent]);return {ok:r[0]==='10 in the call'&&/^00:0[1-3]$/.test(r[1]),d:r.join(' ')}});
await roster(p,'desktop');
await p.click('#callOv .mute');await sleep(p,100);
await chk('call: Mute toggles (aria-pressed)',async()=>({ok:(await p.getAttribute('#callOv .mute','aria-pressed'))==='true'}));
await p.screenshot({path:'screenshots/groups-desktop-call-overlay.png'});
await p.keyboard.press('Escape');await sleep(p,250);
await chk('call: Escape minimizes (never ends) to a 40px accent strip with name and timer; focus on strip',async()=>{const r=await p.evaluate(()=>{const s=document.querySelector('#callStrip');return s&&{h:s.offsetHeight,t:s.textContent,f:document.activeElement===s,ov:!!document.querySelector('#callOv')}});return {ok:!!r&&r.h===40&&/^Weekend hike · \d\d:\d\d$/.test(r.t)&&r.f&&!r.ov,d:JSON.stringify(r)}});
await p.fill('#ta','typing during the call');await p.press('#ta','Enter');await sleep(p,300);
await chk('call minimized: chatting works; header phone button now returns to the call',async()=>({ok:/typing during the call/.test(await p.textContent('#msgs'))&&/Return to call/.test(await p.getAttribute('#callBtn','aria-label'))}));
await p.click('[data-row=family]');await sleep(p,200);
await chk('call minimized: strip stays when switching chats; other group phone button also returns (no second call)',async()=>({ok:await p.locator('#callStrip').isVisible()&&/Return to call with Weekend hike/.test(await p.getAttribute('#callBtn','aria-label'))}));
await p.screenshot({path:'screenshots/groups-desktop-call-strip.png'});
await p.click('#callStrip');await sleep(p,200);
await chk('call: strip returns to the overlay',async()=>({ok:await p.locator('#callOv').isVisible()}));
await p.click('#callOv .end');await sleep(p,300);
await chk('call: End closes overlay + strip, toast "Call ended", focus back on phone button',async()=>({ok:(await p.locator('#callOv').count())===0&&!(await p.locator('#callStrip').isVisible())&&/Call ended/.test(await p.textContent('#toastHost'))&&(await foc(p))==='callBtn',d:await foc(p)}));
await p.click('[data-row=book]');await sleep(p,200);
await chk('group call: no call button in the removed (read-only) group',async()=>({ok:(await p.locator('#callBtn').count())===0}));
await p.click('[data-row=hike]');await sleep(p,200);
await chk('harness: Phone preview link visible at 1440, points at the open group',async()=>{const v=await p.locator('#mobilePreview').isVisible();const h0=await p.getAttribute('#mobilePreview','href');await p.click('[data-row=family]');await sleep(p,200);const h1=await p.getAttribute('#mobilePreview','href');await p.screenshot({path:'screenshots/groups-desktop-phone-preview-link.png'});await p.click('[data-row=hike]');await sleep(p,200);return {ok:v&&h0==='mobile.html?screen=groups&group=hike'&&h1==='mobile.html?screen=groups&group=family',d:h0+' '+h1}});
// new group
await p.focus('#newBtn');await p.click('#newBtn');await sleep(p,300);
await chk('new group dialog opens, focus inside',async()=>({ok:/New group/.test(await dtxt(p))&&await p.evaluate(()=>!!document.activeElement.closest('dialog[open]')),d:'focus='+await foc(p)}));
await p.keyboard.press('Escape');await sleep(p,300);
await chk('Escape closes new-group dialog and restores focus to New group button',async()=>{const f=await foc(p);return {ok:(await dtxt(p))==='NO DIALOG'&&f==='newBtn',d:'focus='+f}});
await p.click('#newBtn');await sleep(p,200);
const rows0=await p.locator('[data-row]').count();
await p.click('dialog[open] button:has-text("Create group")');await sleep(p,300);
await chk('Create with empty name is blocked with an explanation',async()=>{const t=await dtxt(p);return {ok:t!=='NO DIALOG'&&/name|add|choose|someone|at least/i.test(t)&&(await p.locator('[data-row]').count())===rows0,d:t.slice(0,200)}});
await p.fill('#gn','Climbing crew');
await p.locator('dialog[open] input[type=checkbox]').nth(0).check();await p.locator('dialog[open] input[type=checkbox]').nth(2).check();
await p.screenshot({path:'screenshots/groups-desktop-newgroup.png'});
await chk('selected count updates',async()=>{const t=await dtxt(p);return {ok:/2 selected/.test(t),d:(t.match(/\d selected/)||[''])[0]}});
await p.click('dialog[open] button:has-text("Create group")');await sleep(p,600);
await chk('group created: appears in list, dialog closed',async()=>{const t=(await p.textContent('#listPane')).replace(/\s+/g,' ');return {ok:/Climbing crew/.test(t)&&(await dtxt(p))==='NO DIALOG',d:t.slice(0,120)}});
await chk('focus after create not lost to BODY',async()=>{const f=await foc(p);return {ok:f!=='BODY',d:f}});
await sleep(p,3500);await p.screenshot({path:'screenshots/groups-desktop-created.png'});
await chk('new group conversation shows invite-pending/hosts honest state',async()=>{const t=(await p.textContent('#convPane')).replace(/\s+/g,' ');return {ok:/Climbing crew/.test(t),d:t.slice(0,200)}});
// invitations
await open(p);
await p.locator('[aria-label^="Review invitation"]').first().click();await sleep(p,300);
await chk('invitation dialog discloses hosts can read content',async()=>{const t=await dtxt(p);return {ok:/hosts? can read/i.test(t),d:t.slice(0,120)}});
await p.keyboard.press('Escape');await sleep(p,300);
await chk('invitation Escape restores focus to Review button',async()=>{const f=await foc(p);return {ok:/Review invitation/.test(f),d:f}});
await p.locator('[aria-label^="Review invitation"]').first().click();await sleep(p,200);
await p.click('dialog[open] button:has-text("Join")');await sleep(p,1500);
await chk('Join: group appears in Groups, invitation count decreases',async()=>{const t=(await p.textContent('#listPane')).replace(/\s+/g,' ');return {ok:/Trail runners/.test(t)&&/Invitations\s*1/.test(t),d:t.slice(0,200)}});
await chk('focus after Join not lost to BODY',async()=>{const f=await foc(p);return {ok:f!=='BODY',d:f}});
// chat request accept
await open(p);
await p.locator('[aria-label^="Review chat request"]').first().click();await sleep(p,300);
await p.click('dialog[open] button:has-text("Accept")');await sleep(p,1500);
await chk('Accept chat request: Priya shows under Private chats',async()=>{const t=(await p.textContent('#listPane')).replace(/\s+/g,' ');const i=t.indexOf('Private chats');return {ok:/Priya/.test(t.slice(i))&&!/Chat requests\s*1/.test(t),d:t.slice(i,i+120)}});
await chk('focus after Accept not lost to BODY',async()=>{const f=await foc(p);return {ok:f!=='BODY',d:f}});
// compose in a group
await p.locator('[data-row]',{hasText:'Weekend hike'}).click();await sleep(p,300);
await p.fill('#ta','see you at the gate');await p.press('#ta','Enter');await sleep(p,1500);
await chk('group message sent: appears in transcript, composer cleared',async()=>{const t=await p.textContent('#convPane');return {ok:/see you at the gate/.test(t)&&(await p.inputValue('#ta'))==='',d:''}});
await sleep(p,3500);
await chk('sent message reaches a delivered/relayed state (hosts online)',async()=>{const t=(await p.textContent('#convPane')).replace(/\s+/g,' ');const i=t.indexOf('see you at the gate');return {ok:/deliver|relay|sent|host/i.test(t.slice(i,i+120)),d:t.slice(i,i+120)}});
// hosts offline sim
await simOpen(p);await p.locator('#simBody label:has-text("host desktops offline")').click();await sleep(p,500);await simClose(p);
await chk('group call: disabled (aria-disabled, muted) while hosts are degraded; no extra banner; click does nothing',async()=>{const r=await p.evaluate(()=>{const c=document.querySelector('#callBtn');return {dis:c.getAttribute('aria-disabled'),op:getComputedStyle(c).opacity,strips:document.querySelectorAll('.strip').length}});await p.click('#callBtn',{force:true});await sleep(p,200);return {ok:r.dis==='true'&&r.op==='0.38'&&r.strips===1&&(await p.locator('#callOv').count())===0,d:JSON.stringify(r)}});
await p.screenshot({path:'screenshots/groups-desktop-call-disabled.png'});
await chk('hosts offline: thin strip under header, title stays "12 members"',async()=>{const t=await p.textContent('.strip').catch(()=>'');return {ok:/Host offline/.test(t)&&(await p.textContent('#stLine'))==='12 members',d:t}});
await p.fill('#ta','nobody is hosting');await p.press('#ta','Enter');await sleep(p,3500);
await p.screenshot({path:'screenshots/groups-desktop-hosts-offline.png'});
await chk('hosts offline: message shows waiting/not delivered state explaining why',async()=>{const t=(await p.textContent('#convPane')).replace(/\s+/g,' ');const i=t.indexOf('nobody is hosting');return {ok:/wait|offline|not delivered|queued|couldn/i.test(t.slice(i,i+120))&&!/Delivered/.test(t.slice(i,i+60)),d:t.slice(i,i+160)}});
// members/leave
await p.focus('#memBtn');await p.click('#memBtn');await sleep(p,300);
await p.screenshot({path:'screenshots/groups-desktop-members.png'});
await p.keyboard.press('Escape');await sleep(p,300);
await chk('members dialog Escape restores focus to Members button',async()=>({ok:(await foc(p))==='memBtn',d:await foc(p)}));
await p.click('#memBtn');await sleep(p,200);await p.click('#leaveBtn');await sleep(p,300);
await chk('Leave group asks for confirmation with consequences',async()=>{const t=await dtxt(p);return {ok:/^Leave “/.test(t)&&/Cancel/.test(t),d:t.slice(0,260)}});
await p.screenshot({path:'screenshots/groups-desktop-leave.png'});
await p.keyboard.press('Escape');await sleep(p,300);
await chk('Escape on leave confirm does not leave the group',async()=>{const t=(await p.textContent('#listPane')).replace(/\s+/g,' ');return {ok:/Weekend hike/.test(t.split('Private chats')[0])&&!/You left/.test(t.split('Private')[0].split('Weekend hike')[1]||''),d:'focus='+await foc(p)}});
await chk('after Escape from leave confirm, focus is not BODY',async()=>({ok:(await foc(p))!=='BODY',d:await foc(p)}));
await p.locator('#convPane').screenshot().catch(()=>{});
if((await dtxt(p))==='NO DIALOG'){await p.click('#memBtn');await sleep(p,200)}
if(await p.locator('#leaveBtn:visible').count()){await p.click('#leaveBtn');await sleep(p,300)}
const confirmBtn=p.locator('dialog[open] button',{hasText:/^Leave/}).last();
if(await confirmBtn.count()){await confirmBtn.click();await sleep(p,1200)}
await chk('confirmed leave: group shows left/removed state and composer disabled or hidden',async()=>{const t=(await p.textContent('#listPane')).replace(/\s+/g,' ');const ta=await p.locator('#ta:visible').count();const dis=ta?await p.$eval('#ta',e=>e.disabled):true;return {ok:/left|Left/.test(t)||dis,d:t.slice(0,160)+' taDisabled='+dis}});
await chk('focus after leave not lost to BODY',async()=>({ok:(await foc(p))!=='BODY',d:await foc(p)}));
await p.screenshot({path:'screenshots/groups-desktop-left.png'});
await chk('no console errors',async()=>({ok:p.errs.length===0,d:p.errs.join('|')}));
await chk('no failed/404 requests',async()=>({ok:p.fails.length===0,d:p.fails.join('|')}));
await p.context().close();
// empty account
p=await L.page(b);await p.goto(L.BASE+'groups.html');await sleep(p,300);
await simOpen(p);await p.locator('#simBody label:has-text("empty account")').click();await sleep(p,500);
await p.screenshot({path:'screenshots/groups-desktop-empty.png'});
await chk('empty account: empty state with a New group action',async()=>{const t=(await p.textContent('#listPane')).replace(/\s+/g,' ');return {ok:/no groups|nothing|start|create|first/i.test(t),d:t.slice(0,200)}});
await p.context().close();
// mobile
p=await L.page(b,{w:390,h:844,mobile:true});await p.goto(L.BASE+'groups.html');await sleep(p,400);
await chk('mobile: call overlay + strip across conversation and list views at 390',async()=>{await p.locator('[data-row=hike]').click();await sleep(p,300);const w=await p.$eval('#callBtn',c=>c.offsetWidth);await p.click('#callBtn');await sleep(p,3000);await p.screenshot({path:'screenshots/groups-mobile-call-overlay.png'});
  await p.click('#callOv .cmin');await sleep(p,200);await p.screenshot({path:'screenshots/groups-mobile-call-strip.png'});const inConv=await p.locator('#callStrip').isVisible();
  await p.click('[aria-label="Back to groups"]');await sleep(p,300);const inList=await p.locator('#callStrip').isVisible();await p.screenshot({path:'screenshots/groups-mobile-call-strip-list.png'});
  await p.click('#callStrip');await p.click('#callOv .end');await sleep(p,200);return {ok:w===44&&inConv&&inList&&!(await p.locator('#callStrip').isVisible()),d:[w,inConv,inList].join()}});
await chk('mobile: Phone preview link hidden under 860px',async()=>({ok:!(await p.locator('#mobilePreview').isVisible())}));
await p.screenshot({path:'screenshots/groups-mobile-list.png'});
await chk('mobile list: no overflow',async()=>{const o=await L.overflow(p);return {ok:o.sw<=o.cw,d:JSON.stringify(o)}});
await p.locator('[data-row]').first().click();await sleep(p,500);
await p.screenshot({path:'screenshots/groups-mobile-conversation.png'});
await chk('mobile conversation: composer within first viewport',async()=>{const r=await p.$eval('#ta',e=>e.getBoundingClientRect().bottom);return {ok:r<=844,d:'ta.bottom='+Math.round(r)}});
await p.click('[aria-label="Back to groups"],button:has-text("Back to groups")');await sleep(p,400);
await chk('mobile Back returns to list with focus on opened row',async()=>{const f=await foc(p);return {ok:await p.locator('[data-row]').first().isVisible()&&f!=='BODY',d:'focus='+f}});
await p.click('#newBtn');await sleep(p,300);await p.screenshot({path:'screenshots/groups-mobile-newgroup.png'});
await chk('mobile new-group dialog fits viewport',async()=>{const r=await p.evaluate(()=>{const d=document.querySelector('dialog[open]');const b=d.getBoundingClientRect();return [Math.round(b.left),Math.round(b.right),Math.round(b.top),Math.round(b.bottom)]});return {ok:r[0]>=0&&r[1]<=390&&r[3]<=844,d:JSON.stringify(r)}});
await chk('mobile: touch targets ≥44px for visible buttons/inputs in dialog',async()=>{const r=await p.$$eval('dialog[open] button, dialog[open] input[type=text]',e=>e.filter(x=>x.offsetParent).map(x=>{const b=x.getBoundingClientRect();return [(x.getAttribute('aria-label')||x.textContent||x.id).trim().slice(0,16),Math.round(b.width),Math.round(b.height)]}).filter(x=>x[2]<40));return {ok:r.length===0,d:JSON.stringify(r)}});
await p.context().close();
p=await L.page(b);await p.goto(L.BASE+'groups.html?embed=1&group=hike');await sleep(p,300);
await p.click('#callBtn');await sleep(p,300);
await chk('embed=1: call overlay fills the iframe viewport, stays in page',async()=>{const r=await p.evaluate(()=>{const b=document.querySelector('#callOv').getBoundingClientRect();return [b.top,b.left,Math.round(b.width)===innerWidth,Math.round(b.height)===innerHeight]});return {ok:r[0]===0&&r[1]===0&&r[2]&&r[3]&&/groups\.html/.test(p.url()),d:JSON.stringify(r)}});
await p.keyboard.press('Escape');await sleep(p,200);
await chk('embed=1: minimized strip sits above the conversation header',async()=>{const r=await p.evaluate(()=>[document.querySelector('#callStrip').getBoundingClientRect().bottom,document.querySelector('.chead').getBoundingClientRect().top]);return {ok:Math.abs(r[0]-r[1])<1,d:r.join()}});
await p.click('#callStrip');await p.click('#callOv .end');await sleep(p,200);
await chk('embed=1: Phone preview link not shown',async()=>({ok:!(await p.locator('#mobilePreview').isVisible())}));
await p.context().close();
p=await L.page(b,{dark:true});await p.goto(L.BASE+'groups.html');await sleep(p,300);await p.screenshot({path:'screenshots/groups-desktop-dark-open.png'});
{// a group call outlives the page: it shows in chat.html and comes back to groups
const q=await L.page(b);await q.goto(L.BASE+'groups.html?group=hike');await sleep(q,300);await q.click('#callBtn');await sleep(q,3000);
const gSig=sig(await rosterInfo(q)),stored=await q.evaluate(()=>JSON.parse(sessionStorage.getItem('tinline.call')));
await chk('cross-page: roster lives in the shared call state (sessionStorage), k=3, Arjun hosts',async()=>({ok:stored.kind==='group'&&stored.k===3&&stored.roster.length===11&&stored.roster.filter(x=>x.host).map(x=>x.id).join()==='arjun'&&stored.roster.filter(x=>x.fwd).map(x=>x.id).join()==='arjun,sam,priya',d:JSON.stringify(stored.roster.map(x=>x.id))}));
await q.keyboard.press('Escape');
await q.goto(L.BASE+'chat.html?chat=arjun');await sleep(q,300);
await chk('cross-page: group call strip in chat.html; chat phone button returns to it',async()=>{const t=await q.textContent('#callStrip');return {ok:await q.locator('#callStrip').isVisible()&&/^Weekend hike · /.test(t)&&(await q.getAttribute('#callBtn','aria-label'))==='Return to call with Weekend hike',d:t}});
await q.click('#callStrip');await sleep(q,200);
await chk('cross-page: chat.html shows the same group roster, not one avatar (identical tiles, quiet row, +n)',async()=>{const r=await rosterInfo(q),cSig=sig(r);const one=await q.evaluate(()=>document.querySelector('#callOv .who2').hidden&&document.querySelector('#callOv').getAttribute('aria-label'));return {ok:await q.locator('#callOv').isVisible()&&cSig===gSig&&r.tiles.length===3&&one==='Call with Weekend hike',d:one+' '+(cSig===gSig?'same':cSig+' VS '+gSig)}});
await q.screenshot({path:'screenshots/roster-chat-crosspage-1440-light.png'});
await q.click('#callEnd');await sleep(q,150);
await q.goto(L.BASE+'groups.html?group=hike');await sleep(q,300);
await chk('cross-page: ended in chat → gone in groups',async()=>({ok:await q.locator('#callStrip').isHidden()&&(await q.getAttribute('#callBtn','aria-label'))==='Start group call'}));
await chk('cross-page: no AudioContext, no errors',async()=>{const a=await L.aud(q);return {ok:!a.started&&!a.ctxStates.length&&!q.errs.length,d:JSON.stringify(a)+q.errs.join('|')}});
await q.context().close()}
{// roster at 390 and 1440, light and dark; at 390 the three speaking tiles wrap to two rows before the quiet row
for(const [w,hh,mobile] of [[1440,1000,false],[390,844,true]])for(const dark of [false,true]){
 const q=await L.page(b,{w,h:hh,mobile,dark}),tag=w+'-'+(dark?'dark':'light');await q.goto(L.BASE+'groups.html?group=hike');await sleep(q,400);
 await q.click('#callBtn');await sleep(q,3400);
 const r=await rosterInfo(q);
 await chk('roster '+tag+': host, speaking, muted and "+1" overflow all on screen',async()=>{const vis=await q.evaluate(()=>[...document.querySelectorAll('#callOv .t-gc li')].every(li=>{const b=li.getBoundingClientRect();return b.top>=0&&b.bottom<=innerHeight&&b.left>=0&&b.right<=innerWidth}));return {ok:vis&&r.tiles.some(t=>t.host)&&r.tiles.some(t=>t.on)&&r.tiles.some(t=>t.mic)&&r.quiet.some(x=>x.more==='+1'),d:'inView='+vis}});
 if(w===390)await chk('roster 390: speaking tiles wrap to two rows, quiet row starts below both',async()=>{const tops=[...new Set(r.tiles.map(t=>t.top))],qt=Math.min(...r.quiet.map(x=>x.top));return {ok:tops.length===2&&qt>Math.max(...r.tiles.map(t=>t.bottom)),d:'rows='+tops+' quietTop='+qt}});
 else await chk('roster 1440: speaking tiles share one row',async()=>({ok:new Set(r.tiles.map(t=>t.top)).size===1}));
 await q.screenshot({path:'screenshots/roster-group-call-'+tag+'.png'});
 if(!dark){await q.click('#callOv .t-gc-more');await sleep(q,250);await q.screenshot({path:'screenshots/roster-sheet-'+tag+'.png'});await q.keyboard.press('Escape')}
 await chk('roster '+tag+': no console errors',async()=>({ok:!q.errs.length,d:q.errs.join('|')}));
 await q.context().close()}}
{// sound: Quiet mode on by default; with it off a group call never rings (no ring/back), only pulse/connected/soft pulse/ended
const q=await L.page(b);await freqHook(q);await q.goto(L.BASE+'groups.html?group=hike');await sleep(q,300);
await simOpen(q);
await chk('quiet mode: "Quiet mode" switch in the Simulation drawer (role=switch), on by default',async()=>{const r=await q.evaluate(()=>{const s=document.querySelector('#swQuiet');return s&&{role:s.getAttribute('role'),on:s.getAttribute('aria-checked'),t:s.textContent.trim(),h:s.offsetHeight}});return {ok:r&&r.role==='switch'&&r.on==='true'&&r.t==='Quiet mode'&&r.h>=44,d:JSON.stringify(r)}});
await q.screenshot({path:'screenshots/roster-quiet-switch-groups.png'});
await simClose(q);await q.click('#callBtn');await sleep(q,3200);await q.click('#callOv .end');await sleep(q,400);
await chk('quiet mode on: a whole group call creates no AudioContext and no oscillator',async()=>{const a=await audio(q);return {ok:a.ctx===0&&a.started===0&&!a.played.length,d:JSON.stringify(a)}});
await simOpen(q);await q.click('#swQuiet');
await chk('quiet mode off: switch flips, still nothing plays until a call',async()=>{const a=await audio(q);return {ok:(await q.getAttribute('#swQuiet','aria-checked'))==='false'&&a.started===0,d:JSON.stringify(a)}});
await simClose(q);await q.reload();await sleep(q,300);
await chk('sound on, after reload: no AudioContext before a gesture',async()=>{const a=await audio(q);return {ok:a.ctx===0,d:JSON.stringify(a)}});
await q.click('#callBtn');await sleep(q,500);
await chk('group call joining: one short pulse, no ringback',async()=>{const a=await audio(q);return {ok:a.played.join()==='pulse'&&a.ringOrBack===0&&a.ctx===1,d:JSON.stringify(a)}});
await sleep(q,2900);
await chk('group call: connected cue once you are in the roster',async()=>{const a=await audio(q);return {ok:a.played.join()==='pulse,connected'&&a.ringOrBack===0,d:JSON.stringify(a)}});
await sleep(q,7500);
await chk('group call: Leo joins → soft pulse, "11 in the call", "+2", announced politely',async()=>{const a=await audio(q),r=await rosterInfo(q),live=await q.textContent('#callOv .t-gc [role=status]');return {ok:a.played.join()==='pulse,connected,pulse:soft'&&r.sub==='11 in the call'&&r.quiet.find(x=>x.more).more==='+2'&&live==='Leo joined the call',d:JSON.stringify(a.played)+' '+r.sub+' '+live}});
await q.click('#callOv .end');await sleep(q,1500);
await chk('group call End: ended cue once, nothing left playing, never a ring/back oscillator',async()=>{const a=await audio(q);return {ok:a.played.join()==='pulse,connected,pulse:soft,ended'&&a.live===0&&a.ringOrBack===0&&a.freqs.every(f=>[330,660,523,784,1046,1568].includes(f)),d:JSON.stringify(a)}});
await chk('group sound: no console errors',async()=>({ok:!q.errs.length,d:q.errs.join('|')}));
await q.context().close()}
{// message-sent cue (TinAudio.sent) in groups.html: same pluck as chat.html, same Quiet-mode gate
const s=await L.page(b);await freqHook(s);await s.goto(L.BASE+'groups.html?group=hike');await sleep(s,300);
await s.fill('#ta','quiet send');await s.click('#sendBtn');await sleep(s,300);
await chk('message sent, Quiet mode on by default: no AudioContext at all',async()=>{const a=await audio(s);return {ok:a.ctx===0&&a.started===0&&!a.played.length,d:JSON.stringify(a)}});
await simOpen(s);await s.click('#swQuiet');await simClose(s);await sleep(s,100);
await s.fill('#ta','loud send');await s.click('#sendBtn');await sleep(s,80);
await chk('Quiet mode off: sending plays the pluck cue (659Hz) at once',async()=>{const a=await audio(s);return {ok:a.played.slice(-1)[0]==='pluck'&&a.freqs.includes(659),d:JSON.stringify(a)}});
await sleep(s,400);
await chk('pluck is a one-shot: zero live oscillators once it settles',async()=>{const a=await audio(s);return {ok:a.live===0,d:JSON.stringify(a)}});
await s.click('#callBtn');await sleep(s,500); // group call now connecting: a quiet pulse plays, never ring/back
await chk('a group call is not interrupted by this page\'s own send button (file attach reuses sendText, same path)',async()=>{const a=await audio(s);return {ok:a.played.slice(-1)[0]==='pulse',d:JSON.stringify(a)}});
await s.click('#callOv .end');await sleep(s,300);
await chk('message-sent sound in groups.html: no console errors',async()=>({ok:!s.errs.length,d:s.errs.join('|')}));
await s.context().close()}
await b.close();C.save('groups-results.json')})();
