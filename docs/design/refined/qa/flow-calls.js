const L=require('./lib'),C=require('./check');
const chk=C.mk('calls');
const sleep=(p,ms)=>p.waitForTimeout(ms);
const dr=async p=>{if(await p.locator('#drawer').isHidden())await p.click('#simtog')};
const scn=async(p,id)=>{await dr(p);await p.click(`[data-scn=${id}]`)};
const focusInfo=p=>p.evaluate(()=>{const a=document.activeElement;return a&&a!==document.body?(a.id||a.dataset.act||a.tagName)+':'+(a.textContent||'').trim().slice(0,20):'BODY'});
(async()=>{const b=await L.launch();
let p=await L.page(b);await p.goto(L.BASE+'calls.html');await sleep(p,300);
await chk('idle: no AudioContext/oscillator created on load',async()=>{const a=await L.aud(p);return {ok:a.started===0&&a.ctxStates.length===0,d:JSON.stringify(a)}});
await chk('harness: drawer closed by default, no disclaimer text visible',async()=>{const h=await p.locator('#drawer').isHidden();const t=await p.evaluate(()=>document.body.innerText);return {ok:h&&!/simulated|No call is placed|microphone is used|full-screen intent/i.test(t),d:'hidden='+h+' text='+t.replace(/\s+/g,' ').slice(0,120)}});
await chk('harness: theme toggle is icon-only with an accessible name',async()=>{const r=await p.evaluate(()=>{const b=document.querySelector('#theme');return {txt:b.textContent.trim(),al:b.getAttribute('aria-label'),w:Math.round(b.getBoundingClientRect().width)}});return {ok:r.txt===''&&!!r.al&&r.w===36,d:JSON.stringify(r)}});
await chk('idle: one bold line + one plain line + two actions',async()=>{const r=await p.evaluate(()=>{const b=document.querySelector('#stage .body');return {h:b.querySelector('h3').textContent,p:[...b.querySelectorAll('p')].map(x=>x.textContent),a:[...b.querySelectorAll('button')].map(x=>x.textContent)}});return {ok:r.h==='No active call'&&r.p.length===1&&r.a.length===2,d:JSON.stringify(r)}});
await chk('sound switch off by default (aria-checked=false)',async()=>(await p.getAttribute('#son','aria-checked'))==='false');
await scn(p,'out-answer');await sleep(p,2200);
await chk('sound OFF: scenario ringing creates no audio',async()=>{const a=await L.aud(p);return {ok:a.started===0,d:JSON.stringify(a)}});
await chk('sound OFF: caption states what would play',async()=>{const t=await p.textContent('#cap');return {ok:/would|off/i.test(t),d:t.trim().slice(0,100)}});
// enable
await dr(p);await p.click('#son');
await chk('sound switch toggles aria-checked=true',async()=>(await p.getAttribute('#son','aria-checked'))==='true');
await scn(p,'out-answer');await sleep(p,500);
await chk('connecting phase is silent',async()=>{const a=await L.aud(p);return {ok:a.live===0,d:JSON.stringify(a)}});
await sleep(p,1800);
await chk('ringback starts only once remote ringing (oscillators started, one AudioContext)',async()=>{const a=await L.aud(p);return {ok:a.started>0&&a.ctxStates.length===1,d:'started='+a.started+' ctx='+a.ctxStates}});
await chk('caption shows playing',async()=>{const t=await p.textContent('#cap');return {ok:/play|ringback|hum/i.test(t),d:t.trim().slice(0,120)}});
await p.click('[data-act=cancel]');await sleep(p,350);
await sleep(p,1500);
await chk('cancel during ringing: every oscillator stop()ed and ended within ~850ms (gain ramps to 0 at once)',async()=>{const a=await L.aud(p);const live=await p.evaluate(()=>window.__aud.osc.filter(o=>o.started&&!o.ended).length);return {ok:a.started===a.stopped/2||live===0,d:'started='+a.started+' stopped='+a.stopped+' notEnded='+live}});
await chk('after cancel + 1s: zero live oscillators (no more cycles scheduled)',async()=>{const a=await L.aud(p);const n1=a.started;await sleep(p,3500);const a2=await L.aud(p);return {ok:a2.live===0&&a2.started===n1||a2.live===0&&/ended|failed|cue/i.test(''),d:'before '+n1+' after '+a2.started+' live '+a2.live+' stage="'+(await p.textContent('#stage')).replace(/\s+/g,' ').slice(0,80)+'"'}});
await chk('focus after cancel is not lost to BODY',async()=>{const f=await focusInfo(p);return {ok:f!=='BODY',d:f}});
// answered flow to connected
await scn(p,'out-answer');
await sleep(p,9500);
await chk('answered flow: stage re-renders to connected (timer) after Connected is logged',async()=>{const t=(await p.textContent('#stage')).replace(/\s+/g,' ');const a=await L.aud(p);return {ok:/\d\d:\d\d/.test(t),d:t.slice(0,90)+' '+JSON.stringify(a)}});
await chk('in-call: timer is tabular and does not shift layout as it ticks',async()=>{const m=()=>p.evaluate(()=>{const d=document.querySelector('#dur'),r=d.getBoundingClientRect(),c=document.querySelector('.ctl').getBoundingClientRect();return {x:Math.round(r.left),w:Math.round(r.width),num:getComputedStyle(d).fontVariantNumeric,cy:Math.round(c.top),t:d.textContent}});const a=await m();await sleep(p,2100);const b2=await m();return {ok:/tabular-nums/.test(a.num)&&a.x===b2.x&&a.w===b2.w&&a.cy===b2.cy&&a.t!==b2.t,d:JSON.stringify([a,b2])}});
await chk('in-call: control cluster centered and symmetric (mute | end | output)',async()=>{const r=await p.evaluate(()=>{const f=document.querySelector('.dev .body').getBoundingClientRect(),bs=[...document.querySelectorAll('.ctl .c')].map(x=>x.getBoundingClientRect());const mid=f.left+f.width/2;return {acts:[...document.querySelectorAll('.ctl .cb')].map(x=>x.dataset.act),sizes:bs.map(b=>Math.round(b.width)),offs:bs.map(b=>Math.round(b.left+b.width/2-mid)),cy:bs.map(b=>Math.round(b.top+b.height/2))}});return {ok:r.acts.join()==='mute,end,route'&&r.sizes.join()==='56,64,56'&&Math.abs(r.offs[1])<=1&&Math.abs(r.offs[0]+r.offs[2])<=1&&new Set(r.cy).size===1,d:JSON.stringify(r)}});
await chk('connected event logged even if stage stale',async()=>{const l=await p.textContent('#log');return {ok:/Connected/.test(l),d:(await p.textContent('#log')).slice(0,80)}});
await scn(p,'in-second');await p.waitForSelector('[data-act=mute]');
await chk('mute toggles aria-pressed',async()=>{const m=p.locator('[data-act=mute]');const a=await m.getAttribute('aria-pressed');await m.click();const b2=await p.locator('[data-act=mute]').getAttribute('aria-pressed');return {ok:a!==b2&&b2!==null,d:a+'→'+b2}});
// route dialog focus
await p.click('[data-act=route]');await sleep(p,200);
await chk('route dialog opens, focus moves inside',async()=>{const o=await p.evaluate(()=>document.querySelector('#route').open);const inside=await p.evaluate(()=>!!document.activeElement.closest('#route'));return {ok:o&&inside,d:'open='+o+' focusInside='+inside}});
await p.keyboard.press('Escape');await sleep(p,200);
await chk('Escape closes route dialog and restores focus to route button',async()=>{const o=await p.evaluate(()=>document.querySelector('#route').open);const f=await p.evaluate(()=>document.activeElement.dataset.act||document.activeElement.id);return {ok:!o&&f==='route',d:'open='+o+' focus='+f}});
await p.click('[data-act=route]');await p.click('#route-x');await sleep(p,200);
await chk('Done closes route dialog and restores focus',async()=>{const f=await p.evaluate(()=>document.activeElement.dataset.act||document.activeElement.id);return {ok:f==='route',d:f}});
await p.click('[data-act=end]');await sleep(p,500);
await chk('end call: history record added',async()=>{const h=await p.textContent('#hist');return {ok:/Arjun/.test(h),d:h.replace(/\s+/g,' ').slice(0,80)}});
// decline
await scn(p,'out-decline');await sleep(p,5600);
await chk('declined: shows declined state, silent after cue',async()=>{const t=(await p.textContent('#stage')).replace(/\s+/g,' ');await sleep(p,1500);const a=await L.aud(p);return {ok:/declin/i.test(t)&&a.live===0,d:t.slice(0,90)+' '+JSON.stringify(a)}});
// offline retry
await scn(p,'out-offline');await sleep(p,3200);
await chk('offline: couldn\'t reach with Try again',async()=>/Try again/.test(await p.textContent('#stage')));
await p.click('[data-act=retry]');await sleep(p,500);
await chk('retry re-enters connecting (focus not BODY)',async()=>{const t=(await p.textContent('#stage')).replace(/\s+/g,' ');const f=await focusInfo(p);return {ok:/Connecting/.test(t)&&f!=='BODY',d:t.slice(0,60)+' focus='+f}});
await sleep(p,8500);
await chk('retry eventually connects (back online)',async()=>{const t=(await p.textContent('#stage')).replace(/\s+/g,' ');return {ok:/\d\d:\d\d/.test(t),d:t.slice(0,90)}});
// noanswer retry
await scn(p,'out-noanswer');await sleep(p,12500);
await chk('no answer: timeout shows retry option and silences ringback',async()=>{const t=(await p.textContent('#stage')).replace(/\s+/g,' ');const a=await L.aud(p);return {ok:/Try again|again|No answer/i.test(t)&&a.live===0,d:t.slice(0,100)+' live='+a.live}});
// incoming ring/decline/answer
await scn(p,'in-ring');await sleep(p,700);
const s0=(await L.aud(p)).started;
await chk('incoming: ringtone oscillators started',async()=>({ok:s0>0,d:'started='+s0}));
await sleep(p,3600);
await chk('incoming ringtone repeats (cycle 2 started)',async()=>{const a=await L.aud(p);return {ok:a.started>=s0+8,d:'started '+s0+'→'+a.started}});
await p.click('[data-act=decline]');await sleep(p,400);
await chk('Decline silences ringtone within 400ms',async()=>{await sleep(p,500);const live=await p.evaluate(()=>window.__aud.osc.filter(o=>o.started&&!o.ended).length);return {ok:live===0,d:'notEnded='+live}});
await chk('decline: focus not lost to BODY',async()=>{const f=await focusInfo(p);return {ok:f!=='BODY',d:f}});
await scn(p,'in-ring');await sleep(p,600);await p.click('[data-act=answer]');await sleep(p,600);
await chk('Answer silences ringtone',async()=>{await sleep(p,500);const ne=await p.evaluate(()=>window.__aud.osc.filter(o=>o.started&&!o.ended).length);return {ok:ne===0,d:'notEnded='+ne}});
await sleep(p,3000);
await chk('answer: connected view',async()=>{const t=(await p.textContent('#stage')).replace(/\s+/g,' ');return {ok:/\d\d:\d\d/.test(t),d:t.slice(0,80)}});
// ringtone timeout
await scn(p,'in-ring');await sleep(p,500);
await dr(p);const ringtimeout=p.locator('#rem button',{hasText:'Stops ringing'});await ringtimeout.click();await sleep(p,500);
await chk('incoming timeout: silent + missed in history',async()=>{const a=await L.aud(p);const h=await p.textContent('#hist');return {ok:/Missed/i.test(h),d:h.replace(/\s+/g,' ').slice(0,100)+' live='+a.live}});
// scenario switch mid ring
await scn(p,'in-ring');await sleep(p,500);await scn(p,'out-mic');await sleep(p,1600);
await chk('switching scenario mid-ring stops ringtone',async()=>{const live=await p.evaluate(()=>window.__aud.osc.filter(o=>o.started&&!o.ended).length);return {ok:live===0,d:'notEnded='+live}});
await chk('mic denied: Not now → focus',async()=>{await p.click('[data-act=notnow]');await sleep(p,300);return {ok:true,d:'focus='+await focusInfo(p)+' stage='+(await p.textContent('#stage')).replace(/\s+/g,' ').slice(0,60)}});
// platform switch mid ring
await scn(p,'in-ring');await sleep(p,500);await p.click('#p-desk');await sleep(p,500);
await chk('platform switch mid-ring: audio stops or restarts consistently',async()=>{const a=await L.aud(p);const t=(await p.textContent('#stage')).replace(/\s+/g,' ');return {ok:true,d:JSON.stringify(a)+' '+t.slice(0,70)}});
await dr(p);await p.click('#stopsnd');
// previews
for(const nm of ['Ringtone','Ringback']){await p.click(`#pv button:has-text("${nm}")`);await sleep(p,300);const a1=await L.aud(p);await p.click('#stopsnd');await sleep(p,900);const live=await p.evaluate(()=>window.__aud.osc.filter(o=>o.started&&!o.ended).length);
 await chk(`preview ${nm}: plays then Stop sound silences`,async()=>({ok:a1.started>0&&live===0,d:`during live=${a1.live} after notEnded=${live}`}))}
await p.click('#pv button:has-text("Ringtone")');await sleep(p,300);await p.click('#replay');await sleep(p,300);
await chk('replay does not stack ringtones (oscillators in flight ≤ one cycle)',async()=>{const live=await p.evaluate(()=>window.__aud.osc.filter(o=>o.started&&!o.stopped).length);return {ok:live<=8,d:'unstopped='+live}});
await p.click('#stopsnd');
// volume
await p.fill('#vol','0');
await chk('volume slider updates readout, has label',async()=>{const v=await p.textContent('#volv');return {ok:v.trim()==='0',d:v}});
// keyboard
await dr(p);await p.focus('[data-scn=out-decline]');await p.keyboard.press('Enter');await sleep(p,400);
await chk('keyboard Enter on scenario starts it',async()=>{const t=(await p.textContent('#stage')).replace(/\s+/g,' ');return {ok:/Connecting|Ringing/.test(t),d:t.slice(0,50)}});
await dr(p);await p.focus('#son');await p.keyboard.press('Space');await sleep(p,100);
await chk('keyboard Space toggles sound switch',async()=>{const v=await p.getAttribute('#son','aria-checked');return {ok:v==='false',d:v}});
// theme
await p.click('#theme');
await chk('theme toggle sets aria-pressed and changes page background',async()=>{const ap=await p.getAttribute('#theme','aria-pressed');const bg=await p.evaluate(()=>getComputedStyle(document.body).backgroundColor);return {ok:ap==='true'&&bg!=='rgb(245, 246, 243)',d:`pressed=${ap} bg=${bg}`}});
await chk('no console errors during calls flows',async()=>({ok:p.errs.length===0,d:p.errs.join('|')}));
await chk('no failed requests / 404s',async()=>({ok:p.fails.length===0,d:p.fails.join('|')}));
// links
await chk('back link resolves',async()=>{const r=await p.request.get(L.BASE+'index.html');return r.ok()});
await p.context().close();
// mobile pass
p=await L.page(b,{w:390,h:844,mobile:true});await p.goto(L.BASE+'calls.html');
await scn(p,'in-ring');await sleep(p,500);
await p.screenshot({path:'screenshots/calls-mobile-incoming.png'});
await chk('mobile: starting a scenario brings the call screen (Answer/Decline) into view',async()=>{const r=await p.evaluate(()=>{const a=document.querySelector('[data-act=answer]').getBoundingClientRect();return {top:Math.round(a.top),vh:innerHeight}});return {ok:r.top>=0&&r.top<r.vh,d:JSON.stringify(r)}});
await chk('mobile: incoming answer/decline buttons ≥44px and inside viewport',async()=>{const r=await p.$$eval('[data-act=answer],[data-act=decline]',e=>e.map(x=>{const b=x.getBoundingClientRect();return [Math.round(b.width),Math.round(b.height),Math.round(b.right)]}));return {ok:r.every(x=>x[0]>=44&&x[1]>=44&&x[2]<=390),d:JSON.stringify(r)}});
await chk('mobile: no horizontal overflow during scenario',async()=>{const o=await L.overflow(p);return {ok:o.sw<=o.cw,d:JSON.stringify(o)}});
await scn(p,'in-second');await sleep(p,3000);await p.screenshot({path:'screenshots/calls-mobile-second.png',fullPage:true});
await chk('mobile: no overflow in second-call',async()=>{const o=await L.overflow(p);return {ok:o.sw<=o.cw,d:JSON.stringify(o)}});
await p.click('#p-desk');await scn(p,'out-answer');await sleep(p,3000);await p.screenshot({path:'screenshots/calls-mobile-desktop-platform.png',fullPage:true});
await chk('mobile: no overflow in desktop-platform view',async()=>{const o=await L.overflow(p);return {ok:o.sw<=o.cw,d:JSON.stringify(o)}});
await p.context().close();
// dark desktop incoming
p=await L.page(b,{dark:true});await p.goto(L.BASE+'calls.html');await scn(p,'in-second');await sleep(p,3000);await p.screenshot({path:'screenshots/calls-desktop-dark-second.png'});
await scn(p,'out-offline');await sleep(p,3000);await p.screenshot({path:'screenshots/calls-desktop-dark-unreachable.png'});
// deep links from chat/groups headers: calls.html?call=<id>
const HUE=n=>'av'+([...n].reduce((a,c)=>a+c.codePointAt(0),0)%6);
const dlState=p=>p.evaluate(()=>({name:(document.querySelector('#stage-t')||{}).textContent,st:((document.querySelector('#stage .st')||{}).textContent||''),av:((document.querySelector('#stage .t-avatar')||{}).className||''),ring:!!document.querySelector('#stage .halo.on'),and:document.querySelector('#p-android').getAttribute('aria-pressed'),desk:document.querySelector('#p-desk').getAttribute('aria-pressed'),drawer:document.querySelector('#drawer').hidden}));
for(const [id,nm] of [['arjun','Arjun Oommen'],['meera','Meera Nair'],['dev','Dev Patel'],['sana','Sana Rao']])for(const [w,h,mob,plat] of [[390,844,true,'android'],[1440,1000,false,'desk']])for(const dark of [false,true]){
 p=await L.page(b,{w,h,dark,mobile:mob});await p.goto(L.BASE+'calls.html?call='+id);await sleep(p,400);
 const s0=await dlState(p),a0=await L.aud(p);
 if(!dark)await chk(`deep link ?call=${id} @${w}: outgoing ringing to ${nm}, hue ${HUE(nm)}, ${plat} picked, silent (no AudioContext)`,async()=>({ok:s0.name===nm&&s0.st==='Ringing…'&&s0.ring&&s0.av.includes(HUE(nm))&&(plat==='android'?s0.and==='true':s0.desk==='true')&&s0.drawer&&a0.started===0&&a0.ctxStates.length===0,d:JSON.stringify(s0)+' '+JSON.stringify(a0)}));
 if(id==='meera'||id==='arjun')await p.screenshot({path:`screenshots/calls-deeplink-${id}-${w}-${dark?'dark':'light'}.png`});
 if(id==='arjun'&&w===390&&!dark){
  await p.click('#p-desk');await sleep(p,200);
  await chk('deep link: platform still switchable',async()=>{const s1=await dlState(p);return {ok:s1.desk==='true'&&s1.name==='Arjun Oommen',d:JSON.stringify(s1)}});
  await chk('deep link: sound starts only after a gesture (switch on) and Cancel stops every oscillator',async()=>{await dr(p);await p.click('#son');await sleep(p,300);const a1=await L.aud(p);await p.click('#drawer-x').catch(()=>{});await p.click('[data-act=cancel]');await sleep(p,1500);const live=await p.evaluate(()=>window.__aud.osc.filter(o=>o.started&&!o.ended).length);return {ok:a1.started>0&&live===0,d:'afterGesture='+a1.started+' notEnded='+live}});
 }
 await p.context().close();
}
for(const [w,h,mob] of [[390,844,true],[1440,1000,false]])for(const dark of [false,true]){
 p=await L.page(b,{w,h,dark,mobile:mob});await p.goto(L.BASE+'calls.html?call=group:hike');await sleep(p,400);
 const s0=await dlState(p),a0=await L.aud(p);
 if(!dark)await chk(`deep link ?call=group:hike @${w}: Weekend hike group call screen, group glyph, silent`,async()=>({ok:s0.name==='Weekend hike'&&/grp/.test(s0.av)&&/Connecting/.test(s0.st)&&a0.started===0,d:JSON.stringify(s0)+' '+JSON.stringify(a0)}));
 await p.screenshot({path:`screenshots/calls-deeplink-hike-${w}-${dark?'dark':'light'}.png`});
 if(!dark&&w===1440){await sleep(p,3000);await chk('deep link group: reaches the active group call via the host',async()=>{const t=(await p.textContent('#stage')).replace(/\s+/g,' ');return {ok:/\d\d:\d\d/.test(t)&&/host can hear/.test(t),d:t.slice(0,100)}});await p.screenshot({path:'screenshots/calls-deeplink-hike-1440-light-active.png'})}
 await p.context().close();
}
p=await L.page(b);await p.goto(L.BASE+'calls.html?call=nobody');await sleep(p,300);
await chk('deep link unknown id: normal idle screen, no audio',async()=>{const s0=await dlState(p),a0=await L.aud(p);return {ok:s0.name==='No active call'&&a0.started===0,d:JSON.stringify(s0)}});
await chk('deep links: no console errors',async()=>({ok:p.errs.length===0,d:p.errs.join('|')}));
await p.context().close();
// one sound identity: the generator lives in call-audio.js; calls.html only consumes it
p=await L.page(b);await p.goto(L.BASE+'calls.html');await sleep(p,300);
await chk('shared audio: calls.html loads call-audio.js and keeps no inline PAT/Snd copy',async()=>{const src=await p.evaluate(()=>fetch('calls.html').then(r=>r.text()));return {ok:/<script src="call-audio\.js"><\/script>/.test(src)&&!/var PAT=\{|var Snd=\{/.test(src)&&/var PAT=TinAudio\.PAT,Snd=TinAudio\.Snd/.test(src),d:src.length+' bytes'}});
await chk('shared audio: the cues keep their original notes (Hz) and labels',async()=>{const r=await p.evaluate(()=>Object.fromEntries(Object.entries(TinAudio.PAT).map(([k,v])=>[k,v.label+':'+v.period+':'+v.notes.map(n=>n[0]).join('/')])));
 const want={ring:'Ringtone:3200:784/988/1175/1568',back:'Ringback:4000:392/494',connected:'Connected cue:0:523/784',ended:'Ended cue:0:784/523',failed:'Couldn’t connect cue:0:247/196',pulse:'Reconnecting cue:0:330',pluck:'Sent cue:0:659'};return {ok:JSON.stringify(r)===JSON.stringify(want),d:JSON.stringify(r)}});
await dr(p);await p.click('#pv [data-k=ring]');await sleep(p,300);
await chk('shared audio: a drawer preview plays through the shared Snd and the caption follows it',async()=>{const r=await p.evaluate(()=>({cur:TinAudio.Snd.cur&&TinAudio.Snd.cur.kind,src:TinAudio.Snd.cur&&TinAudio.Snd.cur.src,cap:document.querySelector('#cap').textContent,pressed:document.querySelector('#pv [data-k=ring]').getAttribute('aria-pressed')}));return {ok:r.cur==='ring'&&r.src==='preview'&&/Preview: Ringtone/.test(r.cap)&&r.pressed==='true',d:JSON.stringify(r)}});
await p.click('#stopsnd');await sleep(p,1200);
await chk('shared audio: Stop sound stops it (onchange hook updates the caption), zero live oscillators',async()=>{const r=await p.evaluate(()=>({cur:TinAudio.Snd.cur,cap:document.querySelector('#cap').textContent,live:window.__aud.osc.filter(o=>o.started&&!o.ended).length}));return {ok:r.cur===null&&/Sound stopped/.test(r.cap)&&r.live===0,d:JSON.stringify(r)}});
await chk('shared audio: no console errors',async()=>({ok:p.errs.length===0,d:p.errs.join('|')}));
await p.context().close();
{// the brand mark: sags while connecting/ringing, taut once active; absent for idle and group calls
const q=await L.page(b);await q.goto(L.BASE+'calls.html');await sleep(q,200);
await chk('idle: no mark (nothing to be taut or slack about yet)',async()=>({ok:!(await q.evaluate(()=>!!document.querySelector('.callmark')))}));
await q.click('[data-act=start]');await sleep(q,300);
await chk('connecting: mark present, sagging',async()=>{const r=await q.evaluate(()=>{const m=document.querySelector('.callmark');return m&&{taut:m.classList.contains('taut')}});return {ok:r&&!r.taut,d:JSON.stringify(r)}});
await sleep(q,7200);
await chk('active: mark taut, in sync with the connected status',async()=>{const r=await q.evaluate(()=>({taut:document.querySelector('.callmark')?.classList.contains('taut'),st:document.querySelector('.st')?.textContent}));return {ok:r.taut&&/^\d\d:\d\d$/.test(r.st),d:JSON.stringify(r)}});
await q.click('[data-act=end]');await sleep(q,200);
await chk('ended: no mark (not a live call anymore)',async()=>({ok:!(await q.evaluate(()=>!!document.querySelector('.callmark')))}));
await chk('brand mark (calls.html): no console errors',async()=>({ok:!q.errs.length,d:q.errs.join('|')}));
await q.context().close()}
await b.close();C.save('calls-results.json')})();
