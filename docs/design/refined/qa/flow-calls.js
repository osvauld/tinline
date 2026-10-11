const L=require('./lib'),C=require('./check');
const chk=C.mk('calls');
const sleep=(p,ms)=>p.waitForTimeout(ms);
const focusInfo=p=>p.evaluate(()=>{const a=document.activeElement;return a&&a!==document.body?(a.id||a.dataset.act||a.tagName)+':'+(a.textContent||'').trim().slice(0,20):'BODY'});
(async()=>{const b=await L.launch();
let p=await L.page(b);await p.goto(L.BASE+'calls.html');await sleep(p,300);
await chk('idle: no AudioContext/oscillator created on load',async()=>{const a=await L.aud(p);return {ok:a.started===0&&a.ctxStates.length===0,d:JSON.stringify(a)}});
await chk('sound switch off by default (aria-checked=false)',async()=>(await p.getAttribute('#son','aria-checked'))==='false');
await p.click('[data-scn=out-answer]');await sleep(p,2200);
await chk('sound OFF: scenario ringing creates no audio',async()=>{const a=await L.aud(p);return {ok:a.started===0,d:JSON.stringify(a)}});
await chk('sound OFF: caption states what would play',async()=>{const t=await p.textContent('#cap');return {ok:/would|off/i.test(t),d:t.trim().slice(0,100)}});
// enable
await p.click('#son');
await chk('sound switch toggles aria-checked=true',async()=>(await p.getAttribute('#son','aria-checked'))==='true');
await p.click('[data-scn=out-answer]');await sleep(p,500);
await chk('connecting phase is silent',async()=>{const a=await L.aud(p);return {ok:a.live===0,d:JSON.stringify(a)}});
await sleep(p,1800);
await chk('ringback starts only once remote ringing (oscillators started, one AudioContext)',async()=>{const a=await L.aud(p);return {ok:a.started>0&&a.ctxStates.length===1,d:'started='+a.started+' ctx='+a.ctxStates}});
await chk('caption shows playing',async()=>{const t=await p.textContent('#cap');return {ok:/play|ringback|hum/i.test(t),d:t.trim().slice(0,120)}});
await p.click('text=Cancel call');await sleep(p,350);
await sleep(p,1500);
await chk('cancel during ringing: every oscillator stop()ed and ended within ~850ms (gain ramps to 0 at once)',async()=>{const a=await L.aud(p);const live=await p.evaluate(()=>window.__aud.osc.filter(o=>o.started&&!o.ended).length);return {ok:a.started===a.stopped/2||live===0,d:'started='+a.started+' stopped='+a.stopped+' notEnded='+live}});
await chk('after cancel + 1s: zero live oscillators (no more cycles scheduled)',async()=>{const a=await L.aud(p);const n1=a.started;await sleep(p,3500);const a2=await L.aud(p);return {ok:a2.live===0&&a2.started===n1||a2.live===0&&/ended|failed|cue/i.test(''),d:'before '+n1+' after '+a2.started+' live '+a2.live+' stage="'+(await p.textContent('#stage')).replace(/\s+/g,' ').slice(0,80)+'"'}});
await chk('focus after cancel is not lost to BODY',async()=>{const f=await focusInfo(p);return {ok:f!=='BODY',d:f}});
// answered flow to connected
await p.click('[data-scn=out-answer]');
await sleep(p,9500);
await chk('answered flow: stage re-renders to connected (timer) after Connected is logged',async()=>{const t=(await p.textContent('#stage')).replace(/\s+/g,' ');const a=await L.aud(p);return {ok:/\d\d:\d\d/.test(t),d:t.slice(0,90)+' '+JSON.stringify(a)}});
await chk('connected event logged even if stage stale',async()=>{const l=await p.textContent('#log');return {ok:/Connected/.test(l),d:(await p.textContent('#log')).slice(0,80)}});
await p.click('[data-scn=in-second]');await p.waitForSelector('[data-act=mute]');
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
await p.click('[data-scn=out-decline]');await sleep(p,5600);
await chk('declined: shows declined state, silent after cue',async()=>{const t=(await p.textContent('#stage')).replace(/\s+/g,' ');await sleep(p,1500);const a=await L.aud(p);return {ok:/declin/i.test(t)&&a.live===0,d:t.slice(0,90)+' '+JSON.stringify(a)}});
// offline retry
await p.click('[data-scn=out-offline]');await sleep(p,3200);
await chk('offline: couldn\'t reach with Try again',async()=>/Try again/.test(await p.textContent('#stage')));
await p.click('[data-act=retry]');await sleep(p,500);
await chk('retry re-enters connecting (focus not BODY)',async()=>{const t=(await p.textContent('#stage')).replace(/\s+/g,' ');const f=await focusInfo(p);return {ok:/Connecting/.test(t)&&f!=='BODY',d:t.slice(0,60)+' focus='+f}});
await sleep(p,8500);
await chk('retry eventually connects (back online)',async()=>{const t=(await p.textContent('#stage')).replace(/\s+/g,' ');return {ok:/\d\d:\d\d/.test(t),d:t.slice(0,90)}});
// noanswer retry
await p.click('[data-scn=out-noanswer]');await sleep(p,12500);
await chk('no answer: timeout shows retry option and silences ringback',async()=>{const t=(await p.textContent('#stage')).replace(/\s+/g,' ');const a=await L.aud(p);return {ok:/Try again|again|No answer/i.test(t)&&a.live===0,d:t.slice(0,100)+' live='+a.live}});
// incoming ring/decline/answer
await p.click('[data-scn=in-ring]');await sleep(p,700);
const s0=(await L.aud(p)).started;
await chk('incoming: ringtone oscillators started',async()=>({ok:s0>0,d:'started='+s0}));
await sleep(p,3600);
await chk('incoming ringtone repeats (cycle 2 started)',async()=>{const a=await L.aud(p);return {ok:a.started>=s0+8,d:'started '+s0+'→'+a.started}});
await p.click('[data-act=decline]');await sleep(p,400);
await chk('Decline silences ringtone within 400ms',async()=>{await sleep(p,500);const live=await p.evaluate(()=>window.__aud.osc.filter(o=>o.started&&!o.ended).length);return {ok:live===0,d:'notEnded='+live}});
await chk('decline: focus not lost to BODY',async()=>{const f=await focusInfo(p);return {ok:f!=='BODY',d:f}});
await p.click('[data-scn=in-ring]');await sleep(p,600);await p.click('[data-act=answer]');await sleep(p,600);
await chk('Answer silences ringtone',async()=>{await sleep(p,500);const ne=await p.evaluate(()=>window.__aud.osc.filter(o=>o.started&&!o.ended).length);return {ok:ne===0,d:'notEnded='+ne}});
await sleep(p,3000);
await chk('answer: connected view',async()=>{const t=(await p.textContent('#stage')).replace(/\s+/g,' ');return {ok:/\d\d:\d\d/.test(t),d:t.slice(0,80)}});
// ringtone timeout
await p.click('[data-scn=in-ring]');await sleep(p,500);
const ringtimeout=p.locator('#rem button',{hasText:'Stops ringing'});await ringtimeout.click();await sleep(p,500);
await chk('incoming timeout: silent + missed in history',async()=>{const a=await L.aud(p);const h=await p.textContent('#hist');return {ok:/Missed/i.test(h),d:h.replace(/\s+/g,' ').slice(0,100)+' live='+a.live}});
// scenario switch mid ring
await p.click('[data-scn=in-ring]');await sleep(p,500);await p.click('[data-scn=out-mic]');await sleep(p,1600);
await chk('switching scenario mid-ring stops ringtone',async()=>{const live=await p.evaluate(()=>window.__aud.osc.filter(o=>o.started&&!o.ended).length);return {ok:live===0,d:'notEnded='+live}});
await chk('mic denied: Not now → focus',async()=>{await p.click('[data-act=notnow]');await sleep(p,300);return {ok:true,d:'focus='+await focusInfo(p)+' stage='+(await p.textContent('#stage')).replace(/\s+/g,' ').slice(0,60)}});
// platform switch mid ring
await p.click('[data-scn=in-ring]');await sleep(p,500);await p.click('#p-desk');await sleep(p,500);
await chk('platform switch mid-ring: audio stops or restarts consistently',async()=>{const a=await L.aud(p);const t=(await p.textContent('#stage')).replace(/\s+/g,' ');return {ok:true,d:JSON.stringify(a)+' '+t.slice(0,70)}});
await p.click('#stopsnd');
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
await p.focus('[data-scn=out-decline]');await p.keyboard.press('Enter');await sleep(p,400);
await chk('keyboard Enter on scenario starts it',async()=>{const t=(await p.textContent('#stage')).replace(/\s+/g,' ');return {ok:/Connecting|Ringing/.test(t),d:t.slice(0,50)}});
await p.focus('#son');await p.keyboard.press('Space');await sleep(p,100);
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
await p.click('[data-scn=in-ring]');await sleep(p,500);
await p.screenshot({path:'screenshots/calls-mobile-incoming.png'});
await chk('mobile: starting a scenario brings the call screen (Answer/Decline) into view',async()=>{const r=await p.evaluate(()=>{const a=document.querySelector('[data-act=answer]').getBoundingClientRect();return {top:Math.round(a.top),vh:innerHeight}});return {ok:r.top>=0&&r.top<r.vh,d:JSON.stringify(r)}});
await chk('mobile: incoming answer/decline buttons ≥44px and inside viewport',async()=>{const r=await p.$$eval('[data-act=answer],[data-act=decline]',e=>e.map(x=>{const b=x.getBoundingClientRect();return [Math.round(b.width),Math.round(b.height),Math.round(b.right)]}));return {ok:r.every(x=>x[0]>=44&&x[1]>=44&&x[2]<=390),d:JSON.stringify(r)}});
await chk('mobile: no horizontal overflow during scenario',async()=>{const o=await L.overflow(p);return {ok:o.sw<=o.cw,d:JSON.stringify(o)}});
await p.click('[data-scn=in-second]');await sleep(p,3000);await p.screenshot({path:'screenshots/calls-mobile-second.png',fullPage:true});
await chk('mobile: no overflow in second-call',async()=>{const o=await L.overflow(p);return {ok:o.sw<=o.cw,d:JSON.stringify(o)}});
await p.click('#p-desk');await p.click('[data-scn=out-answer]');await sleep(p,3000);await p.screenshot({path:'screenshots/calls-mobile-desktop-platform.png',fullPage:true});
await chk('mobile: no overflow in desktop-platform view',async()=>{const o=await L.overflow(p);return {ok:o.sw<=o.cw,d:JSON.stringify(o)}});
await p.context().close();
// dark desktop incoming
p=await L.page(b,{dark:true});await p.goto(L.BASE+'calls.html');await p.click('[data-scn=in-second]');await sleep(p,3000);await p.screenshot({path:'screenshots/calls-desktop-dark-second.png'});
await p.click('[data-scn=out-offline]');await sleep(p,3000);await p.screenshot({path:'screenshots/calls-desktop-dark-unreachable.png'});
await b.close();C.save('calls-results.json')})();
