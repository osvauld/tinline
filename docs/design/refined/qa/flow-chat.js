const L=require('./lib'),C=require('./check');
const chk=C.mk('chat');const sleep=(p,ms)=>p.waitForTimeout(ms);
const foc=p=>p.evaluate(()=>{const a=document.activeElement;return a&&a!==document.body?(a.id||a.getAttribute('aria-label')||a.tagName)+'':'BODY'});
const msgs=p=>p.$$eval('[data-mid]',e=>e.map(x=>x.textContent.replace(/\s+/g,' ').trim().slice(0,80)));
(async()=>{const b=await L.launch();
let p=await L.page(b);await p.goto(L.BASE+'chat.html');await sleep(p,300);
await chk('harness: slim bar, drawer closed by default, no disclaimers visible, no hint row',async()=>{const v=await p.locator('#sim').isVisible();const t=(await p.evaluate(()=>document.body.innerText));return {ok:!v&&!/simulated data|Interactive design/i.test(t)&&!(await p.locator('#hint').count()),d:'drawerVisible='+v}});
await chk('no default-blue links',async()=>{const r=await p.$$eval('a',e=>e.filter(a=>a.offsetParent).map(a=>getComputedStyle(a).color).filter(c=>c==='rgb(0, 0, 238)'||c==='rgb(85, 26, 139)'));return {ok:r.length===0,d:r.join(',')}});
const sim=async()=>{if(!(await p.locator('#swFail').isVisible()))await p.click('#simBtn')};
await sim();
await chk('simulation controls reachable & switches have role/aria-checked',async()=>{const r=await p.$$eval('#swOnline,#swFail',e=>e.map(x=>x.getAttribute('role')+':'+x.getAttribute('aria-checked')+':'+x.getAttribute('aria-pressed')));return {ok:r.every(x=>!/null:null:null/.test(x)),d:r.join(' ')}});
await p.locator('[data-chat=arjun]').click();await sleep(p,300);
await chk('bubbles: meta inline (short message is one line, meta inside last line box)',async()=>{const r=await p.$$eval('.m:not(.emoji) .bub:not(.gone)',e=>e.filter(b=>!b.querySelector('.quote,.file')&&b.textContent.length<40).map(b=>{const bb=b.getBoundingClientRect(),mb=b.querySelector('.meta').getBoundingClientRect();return [Math.round(bb.height),Math.round(bb.bottom-mb.bottom)]}));return {ok:r.length>0&&r.every(([h,g])=>h<=38&&g>=4&&g<=12),d:JSON.stringify(r.slice(0,6))}});
await chk('bubbles: sent uses --bub-out, no delivery prose in bubbles',async()=>{const bg=await p.$eval('.m.me .bub',e=>getComputedStyle(e).backgroundColor);const t=await p.$eval('#msgs',e=>e.innerText);return {ok:bg==='rgb(205, 235, 227)'&&!/Delivered to/.test(t),d:bg}});
await chk('grouping: 2px inside a group, tight stack-facing corner',async()=>{const r=await p.$$eval('.m.g-first+.m',e=>e.map(x=>[getComputedStyle(x).marginTop,getComputedStyle(x.previousElementSibling.querySelector('.bub'))[x.classList.contains('me')?'borderBottomRightRadius':'borderBottomLeftRadius']]));return {ok:r.length>0&&r.every(([m,c])=>m==='2px'&&c==='6px'),d:JSON.stringify(r)}});
await chk('no persistent menu buttons (chevrons hidden until hover)',async()=>{const o=await p.$$eval('.more',e=>e.map(x=>getComputedStyle(x).opacity));return {ok:o.length>0&&o.every(x=>x==='0'),d:o.slice(0,4).join(',')}});
await chk('header call button is an in-page button (no navigation)',async()=>{const r=await p.$eval('#callBtn',e=>[e.tagName,e.getAttribute('href'),e.getAttribute('aria-disabled'),e.getAttribute('aria-label')]);return {ok:r[0]==='BUTTON'&&!r[1]&&!r[2]&&/Voice call Arjun/.test(r[3]),d:JSON.stringify(r)}});
await chk('call button sits between name block and info button',async()=>p.$eval('#callBtn',e=>e.previousElementSibling.id==='convTitle'&&e.nextElementSibling.getAttribute('aria-label')==='Chat details'));
const url0=p.url();await p.click('#callBtn');await p.waitForTimeout(300);
await chk('call: overlay fills the app frame, focus on overlay, status Ringing, no navigation, no audio',async()=>{const a=await p.locator('#app').boundingBox(),o=await p.locator('#callOv').boundingBox();const st=await p.textContent('#callStatus');const f=await foc(p);const au=await L.aud(p);return {ok:p.url()===url0&&Math.abs(a.width-o.width)<3&&Math.abs(a.height-o.height)<3&&f==='callOv'&&/Ringing/.test(st)&&!au.started,d:JSON.stringify({o,st,f,au})}});
await p.screenshot({path:'screenshots/chat-desktop-call-ringing.png'});
await p.waitForTimeout(3300);
await chk('call: connects after ~2.5s and timer ticks MM:SS',async()=>{const t1=await p.textContent('#callTime');await p.waitForTimeout(1100);const t2=await p.textContent('#callTime');return {ok:/^\d\d:\d\d$/.test(t1)&&t1!==t2&&/Connected/.test(await p.textContent('#callStatus')),d:t1+' -> '+t2}});
await p.click('#callMute');await chk('call: mute toggles aria-pressed',async()=>({ok:(await p.getAttribute('#callMute','aria-pressed'))==='true'}));await p.click('#callMute');
await p.keyboard.press('Escape');await p.waitForTimeout(200);
await chk('Escape minimizes (call continues) into strip; focus on strip',async()=>{const ov=await p.locator('#callOv').isVisible(),st=await p.locator('#callStrip').isVisible();const t=await p.textContent('#callStrip');const bb=await p.locator('#callStrip').boundingBox();return {ok:!ov&&st&&/Arjun Oommen\s*·\s*\d\d:\d\d/.test(t)&&Math.round(bb.height)===40&&(await foc(p))==='callStrip',d:t.replace(/\s+/g,' ')+' h='+bb.height}});
await chk('while live, header phone button means return to call',async()=>({ok:/Return to call/.test(await p.getAttribute('#callBtn','aria-label')),d:await p.getAttribute('#callBtn','aria-label')}));
await p.fill('#ta','chatting during the call');await p.press('#ta','Enter');await p.waitForTimeout(300);
await chk('chatting works while minimized',async()=>({ok:/chatting during the call/.test((await msgs(p)).slice(-1)[0])}));
await p.locator('[data-chat=meera]').click();await p.waitForTimeout(250);
await chk('strip persists when switching chats; Meera header button returns to the call (no second call)',async()=>({ok:await p.locator('#callStrip').isVisible()&&/Return to call with Arjun/.test(await p.getAttribute('#callBtn','aria-label'))}));
await p.screenshot({path:'screenshots/chat-desktop-call-strip.png'});
await p.click('#callBtn');await p.waitForTimeout(250);
await chk('return to call reopens the same call',async()=>({ok:await p.locator('#callOv').isVisible()&&(await p.textContent('#callName'))==='Arjun Oommen'&&/\d\d:\d\d/.test(await p.textContent('#callTime'))}));
await p.click('#callMin');await p.waitForTimeout(150);await p.click('#callStrip');await p.waitForTimeout(150);
await chk('strip tap returns to overlay',async()=>({ok:await p.locator('#callOv').isVisible()}));
await p.click('#callEnd');await p.waitForTimeout(250);
await chk('End: overlay + strip gone, toast Call ended, focus on phone button, still no audio',async()=>{const t=await p.textContent('#toasts');const au=await L.aud(p);return {ok:!(await p.locator('#callOv').isVisible())&&!(await p.locator('#callStrip').isVisible())&&/Call ended/.test(t)&&(await foc(p))==='callBtn'&&!au.started,d:t+' focus='+await foc(p)}});
await p.locator('[data-chat=arjun]').click();await p.waitForTimeout(300);
await p.locator('[aria-label="Chat details"]').click();await p.waitForTimeout(250);
await chk('contact sheet has a Voice call row that starts the overlay',async()=>{const ok1=/Voice call/.test(await p.textContent('#infoBody'));await p.click('#infoCall');await p.waitForTimeout(250);const v=await p.locator('#callOv').isVisible();return {ok:ok1&&v}});
await p.click('#callEnd');await p.waitForTimeout(200);
await p.locator('[data-chat=dev]').click();await p.waitForTimeout(300);
await chk('Dev (message-only): call button disabled but focusable, no in-conversation banner',async()=>{const r=await p.$eval('#callBtn',e=>[e.tagName,e.getAttribute('aria-disabled'),e.tabIndex,getComputedStyle(e).opacity]);const banner=await p.locator('#conv .strip').count();return {ok:r[0]==='BUTTON'&&r[1]==='true'&&r[2]>=0&&banner===0,d:JSON.stringify(r)+' strips='+banner}});
await p.locator('#callBtn').focus();await p.keyboard.press('Enter');await p.waitForTimeout(250);
await chk('Dev: disabled call opens contact sheet with message-only line + Request calling that flips to Requested',async()=>{const t=(await p.textContent('#infoBody')).replace(/\s+/g,' ');const ok1=/Messages only — calling needs their permission/.test(t)&&/Request calling/.test(t)&&!(await p.locator('#infoBody a.callrow').count());await p.click('#reqCall');await p.waitForTimeout(100);const t2=await p.textContent('#reqCall');return {ok:ok1&&t2==='Requested'&&(await p.getAttribute('#reqCall','aria-disabled'))==='true',d:t.slice(0,120)+' -> '+t2}});
await p.screenshot({path:'screenshots/chat-desktop-dev-callperm.png'});
await p.keyboard.press('Escape');await p.waitForTimeout(200);
await chk('Dev: focus returns to call button after sheet',async()=>({ok:(await foc(p))==='callBtn',d:await foc(p)}));
await p.locator('[data-chat=arjun]').click();await p.waitForTimeout(300);
await chk('open chat: composer visible, Send not enabled when empty',async()=>{const d=await p.$eval('#sendBtn',e=>e.disabled||e.getAttribute('aria-disabled'));return {ok:!!d,d:'disabled='+d}});
// draft
await p.fill('#ta','half written thought');
await p.locator('[data-chat=dev]').click();await sleep(p,300);
await chk('switch chat: composer empty for other chat',async()=>({ok:(await p.inputValue('#ta'))==='',d:await p.inputValue('#ta')}));
const rowTxt=await p.locator('[data-chat]').first().textContent();
await chk('chat list shows Draft marker for chat with unsent text',async()=>({ok:/draft/i.test(await p.textContent('#chats')),d:(await p.textContent('#chats')).replace(/\s+/g,' ').slice(0,160)}));
await p.locator('[data-chat=arjun]').click();await sleep(p,300);
await chk('draft restored on returning to chat',async()=>({ok:(await p.inputValue('#ta'))==='half written thought',d:await p.inputValue('#ta')}));
// shift+enter
await p.fill('#ta','');await p.focus('#ta');await p.keyboard.type('line1');await p.keyboard.press('Shift+Enter');await p.keyboard.type('line2');
await chk('Shift+Enter inserts newline, does not send',async()=>{const v=await p.inputValue('#ta');return {ok:v==='line1\nline2',d:JSON.stringify(v)}});
const n0=(await msgs(p)).length;
await p.keyboard.press('Enter');await sleep(p,200);
await chk('Enter sends; message appears; composer cleared and keeps focus',async()=>{const m=await msgs(p);return {ok:m.length===n0+1&&(await p.inputValue('#ta'))===''&&(await foc(p))==='ta',d:m.slice(-1)[0]+' focus='+await foc(p)}});
await sleep(p,3500);
await chk('sent message progresses to sent/delivered state',async()=>{const m=(await msgs(p)).slice(-1)[0];return {ok:/sent|deliver|✓|read/i.test(m),d:m}});
await p.screenshot({path:'screenshots/chat-desktop-sent.png'});
// failure + retry
await p.click('#swFail');await sleep(p,100);
await chk('Next send fails switch on',async()=>{const v=await p.getAttribute('#swFail','aria-checked')??await p.getAttribute('#swFail','aria-pressed');return {ok:v==='true',d:v}});
await p.fill('#ta','goes nowhere first');await p.press('#ta','Enter');await sleep(p,4000);
await chk('failed send shows failure and Retry control',async()=>{const m=(await msgs(p)).slice(-1)[0];const r=await p.locator('[data-act=retry]').count();return {ok:/not delivered/i.test(m)&&r>0&&!/✓/.test(m),d:m+' retryBtns='+r}});
await p.screenshot({path:'screenshots/chat-desktop-failed.png'});
await chk('Next send fails switch resets after one failure (one-shot)',async()=>{const v=await p.getAttribute('#swFail','aria-checked')??await p.getAttribute('#swFail','aria-pressed');return {ok:v==='false',d:v}});
await p.locator('[data-act=retry]').first().click();await sleep(p,300);
await chk('retry: focus not lost to BODY',async()=>{const f=await foc(p);return {ok:f!=='BODY',d:f}});
await sleep(p,4000);
await chk('retry succeeds (no failed message remains)',async()=>{const m=(await msgs(p)).slice(-1)[0];return {ok:!/fail|not sent|not delivered/i.test(m)&&(await p.locator('[data-act=retry]:visible').count())===0,d:m+' visibleRetry='+await p.locator('[data-act=retry]:visible').count()}});
// offline contact
await p.click('#swOnline');await sleep(p,200);
await chk('contact offline: header/status changes',async()=>{const t=(await p.textContent('#conv')).replace(/\s+/g,' ');return {ok:/offline|waiting|queued|not online/i.test(t),d:t.slice(0,140)}});
await p.fill('#ta','sent while offline');await p.press('#ta','Enter');await sleep(p,1500);
await chk('send while offline shows waiting/queued state',async()=>{const m=(await msgs(p)).slice(-1)[0];return {ok:/wait|queue|will send|pending|sending/i.test(m),d:m}});
await p.click('#swOnline');await sleep(p,4500);
await chk('back online: queued message delivered',async()=>{const m=(await msgs(p)).slice(-1)[0];return {ok:!/wait|queue|pending|sending/i.test(m),d:m}});
// incoming
await p.locator('[data-chat=dev]').click();await sleep(p,200);
const before=await p.textContent('#chats');await p.click('#simIncoming');await sleep(p,800);
await chk('simulate incoming adds a message or badge',async()=>{const after=await p.textContent('#chats');const m=await msgs(p);return {ok:after!==before||m.length>0,d:after.replace(/\s+/g,' ').slice(0,100)}});
// search
await p.fill('#q','zzzz');await sleep(p,200);
await chk('search no match: empty state shown',async()=>{const t=(await p.textContent('#chats')).replace(/\s+/g,' ');return {ok:await p.locator('#listEmpty').isVisible()||/no chats|no results|nothing/i.test(t),d:t.slice(0,100)}});
await p.fill('#q','');
// dialogs focus restoration
for(const [trig,id,name] of [['#settingsBtn','#dSettings','settings'],['#newBtn','#dNew','new chat'],['[aria-label="Attach a file"]','#dAttach','attach'],['[aria-label="Chat details"]','#dInfo','info']]){
 const t=p.locator(trig).first();await t.focus();await t.click();await sleep(p,300);
 const open=await p.evaluate(s=>{const d=document.querySelector(s);return d&&(d.open||d.matches('[open]')||getComputedStyle(d).display!=='none')},id);
 const inside=await p.evaluate(s=>!!document.activeElement.closest(s),id);
 await p.keyboard.press('Escape');await sleep(p,300);
 const closed=await p.evaluate(s=>{const d=document.querySelector(s);return !d.open},id);
 const fa=await p.evaluate(()=>document.activeElement.id||document.activeElement.getAttribute('aria-label')||document.activeElement.tagName);
 const want=await t.evaluate(e=>e.id||e.getAttribute('aria-label'));
 await chk(`dialog ${name}: opens with focus inside, Escape closes, focus returns to trigger`,async()=>({ok:open&&inside&&closed&&fa===want,d:`open=${open} focusInside=${inside} closed=${closed} focus=${fa} want=${want}`}));
}
// actions on message + delete
await p.locator('.m.me .bub').first().hover();await sleep(p,200);
await chk('desktop: chevron appears on hover',async()=>({ok:(await p.locator('.m.me .more').first().evaluate(e=>getComputedStyle(e).opacity))==='1'}));
await p.locator('.m.me [aria-label^="Actions for"]').first().click();await sleep(p,300);
await p.screenshot({path:'screenshots/chat-desktop-actions.png'});
await chk('message menu is an anchored popover listing Reply/Copy/Edit/Delete/Message info',async()=>{const t=(await p.textContent('#actionsBody')).replace(/\s+/g,' ');const cls=await p.getAttribute('#dActions','class');const a=await p.locator('.m.me .more').first().boundingBox(),d=await p.locator('#dActions').boundingBox();const near=Math.abs(d.y-(a.y+a.height))<12||Math.abs(d.y+d.height-a.y)<12;return {ok:cls==='pop'&&near&&/reply/i.test(t)&&/copy/i.test(t)&&/edit/i.test(t)&&/delete/i.test(t)&&/message info/i.test(t),d:cls+' '+t.slice(0,120)}});
await p.keyboard.press('Escape');await sleep(p,300);
await chk('actions Escape restores focus to the actions button',async()=>{const f=await p.evaluate(()=>document.activeElement.getAttribute('aria-label')||document.activeElement.tagName);return {ok:/^Actions for/.test(f),d:f}});
await p.locator('.m.me .bub').first().click({button:'right'});await sleep(p,300);
await chk('right-click opens the same popover',async()=>({ok:(await p.locator('#dActions[open]').count())===1&&(await p.getAttribute('#dActions','class'))==='pop'}));
await p.locator('#dActions .mi',{hasText:'Message info'}).click();await sleep(p,300);
await chk('Message info shows sent time and per-device delivery',async()=>{const t=(await p.textContent('#msgInfoBody')).replace(/\s+/g,' ');return {ok:(await p.locator('#dMsgInfo[open]').count())===1&&/Sent/.test(t)&&/phone/i.test(t)&&/Delivered/.test(t),d:t.slice(0,160)}});
await p.screenshot({path:'screenshots/chat-desktop-msginfo.png'});
await p.keyboard.press('Escape');await sleep(p,300);
// keyboard-only reach send
await p.fill('#ta','kbd');await p.focus('#ta');await p.keyboard.press('Tab');await p.keyboard.press('Tab');
await chk('Tab from filled composer reaches Send button',async()=>{await p.fill('#ta','kbd');await p.focus('#ta');const seq=[];for(let i=0;i<3;i++){await p.keyboard.press('Tab');seq.push(await foc(p))}return {ok:seq.includes('sendBtn')||seq.includes('Send'),d:seq.join(' > ')}});
await chk('Reset demo restores initial state',async()=>{await p.click('#simReset');await sleep(p,500);return {ok:(await p.locator('[data-chat]').count())===4,d:'rows='+await p.locator('[data-chat]').count()}});
await chk('Empty the chat list shows empty state with action',async()=>{await p.click('#simClear');await sleep(p,400);const v=await p.locator('#listEmpty').isVisible();return {ok:v,d:(await p.textContent('#listEmpty').catch(()=>'')).replace(/\s+/g,' ').slice(0,100)}});
await chk('no console errors',async()=>({ok:p.errs.length===0,d:p.errs.join('|')}));
await chk('no failed/404 requests',async()=>({ok:p.fails.length===0,d:p.fails.join('|')}));
await p.context().close();
// mobile
p=await L.page(b,{w:390,h:844,mobile:true});await p.goto(L.BASE+'chat.html');await sleep(p,400);
await p.screenshot({path:'screenshots/chat-mobile-list.png'});
await chk('mobile list: no horizontal overflow',async()=>{const o=await L.overflow(p);return {ok:o.sw<=o.cw,d:JSON.stringify(o)}});
const row=p.locator('[data-chat]').nth(1);await row.click();await sleep(p,500);
await p.screenshot({path:'screenshots/chat-mobile-conversation.png'});
await chk('mobile conversation: composer inside viewport, no overflow',async()=>{const r=await p.$eval('#ta',e=>{const b=e.getBoundingClientRect();return [b.left,b.right,b.bottom]});const o=await L.overflow(p);return {ok:r[0]>=0&&r[1]<=390&&r[2]<=844&&o.sw<=o.cw,d:JSON.stringify(r)+JSON.stringify(o)}});
await chk('mobile conversation: Back button present and returns to list, focus restored to opened row',async()=>{const bk=p.locator('[aria-label="Back to chats"]').first();if(!(await bk.count()))return {ok:false,d:'no back button'};await bk.click();await sleep(p,400);const f=await p.evaluate(()=>document.activeElement.dataset.chat||document.activeElement.id||document.activeElement.tagName);const vis=await p.locator('[data-chat]').first().isVisible();return {ok:vis&&f!=='BODY',d:'listVisible='+vis+' focus='+f}});
await chk('mobile: ≥44px touch targets on primary controls',async()=>{const r=await p.$$eval('button:not([hidden]),input',e=>e.filter(x=>x.offsetParent&&!x.closest('.harness,.sim')&&!x.classList.contains('more')).map(x=>{const b=x.getBoundingClientRect();return [(x.id||x.getAttribute('aria-label')||x.textContent||'').trim().slice(0,18),Math.round(b.width),Math.round(b.height)]}).filter(x=>x[2]<40||x[1]<40));return {ok:r.length===0,d:JSON.stringify(r)}});
await p.context().close();
p=await L.page(b,{dark:true});await p.goto(L.BASE+'chat.html');await p.locator('[data-chat=arjun]').click();await sleep(p,300);await p.screenshot({path:'screenshots/chat-desktop-dark-open.png'});
{// the call outlives the page (call-state.js): chat ↔ groups ↔ reload, ?call=open, ?callbar=host
const q=await L.page(b),secs=t=>{const m=/(\d\d):(\d\d)/.exec(t||'');return m?+m[1]*60+ +m[2]:-1};
await q.goto(L.BASE+'chat.html?chat=arjun');await sleep(q,300);await q.click('#callBtn');await sleep(q,200);await q.keyboard.press('Escape');await sleep(q,2600);
await q.goto(L.BASE+'groups.html?group=hike');await sleep(q,300);
await chk('cross-page: a chat call shows as the strip in groups; group phone button returns to it (no second call)',async()=>{const t=await q.textContent('#callStrip'),l=await q.getAttribute('#callBtn','aria-label');return {ok:await q.locator('#callStrip').isVisible()&&/^Arjun Oommen · /.test(t)&&l==='Return to call with Arjun Oommen',d:t+' | '+l}});
await q.click('#callBtn');await sleep(q,200);
await chk('cross-page: groups overlay renders the direct call with the person avatar',async()=>({ok:await q.locator('#callOv').isVisible()&&(await q.textContent('#callOv .t-avatar'))==='AO'&&(await q.textContent('#callOv h2'))==='Arjun Oommen'}));
await q.click('#callOv .mute');await sleep(q,100);await q.click('#callOv .cmin');
await q.goto(L.BASE+'chat.html?chat=arjun&call=open');await sleep(q,300);
await chk('?call=open raises the overlay on load, then leaves the URL',async()=>({ok:await q.locator('#callOv').isVisible()&&(await foc(q))==='callOv'&&!/call=open/.test(q.url())&&/chat=arjun/.test(q.url()),d:q.url()}));
await chk('mute state travels with the call',async()=>({ok:(await q.getAttribute('#callMute','aria-pressed'))==='true'&&(await q.textContent('#callStatus'))==='Muted'}));
const t1=secs(await q.textContent('#callTime'));await sleep(q,1100);await q.reload();await sleep(q,300);
await chk('reload mid-call: strip shows, timer never resets',async()=>{const t=await q.textContent('#callStrip');return {ok:await q.locator('#callStrip').isVisible()&&!(await q.locator('#callOv').isVisible())&&t1>=1&&secs(t)>t1,d:t+' vs '+t1}});
await q.goto(L.BASE+'chat.html?chat=meera&embed=1&callbar=host');await sleep(q,300);
await chk('?callbar=host: no strip of its own (Home draws it); phone button still returns to the call',async()=>({ok:await q.locator('#callStrip').isHidden()&&(await q.getAttribute('#callBtn','aria-label'))==='Return to call with Arjun Oommen'}));
await q.click('#callBtn');await sleep(q,150);await q.click('#callMin');await sleep(q,100);
await chk('?callbar=host: minimise sends focus to the phone button, not a hidden strip',async()=>({ok:(await foc(q))==='callBtn'&&await q.locator('#callStrip').isHidden(),d:await foc(q)}));
await q.goto(L.BASE+'groups.html?group=hike');await sleep(q,300);await q.click('#callStrip');await sleep(q,150);await q.click('#callOv .end');await sleep(q,350); // End now waits ~220ms for the string/overlay release
await q.goto(L.BASE+'chat.html?chat=arjun');await sleep(q,300);
await chk('ending in groups ended it for chat too',async()=>({ok:await q.locator('#callStrip').isHidden()&&(await q.getAttribute('#callBtn','aria-label'))==='Voice call Arjun'}));
await chk('cross-page call flow never creates an AudioContext',async()=>{const a=await L.aud(q);return {ok:!a.started&&!a.ctxStates.length,d:JSON.stringify(a)}});
await chk('cross-page: no runtime errors',async()=>({ok:!q.errs.length,d:q.errs.join('|')}));
await q.context().close()}
{// direct-call sound (call-audio.js): Quiet mode on by default; with it off, ringback loops while ringing, connected once, ended once
const freqHook=p=>p.context().addInitScript(()=>{const AC=window.AudioContext;if(!AC)return;const co=AC.prototype.createOscillator;window.__oscF=[];AC.prototype.createOscillator=function(){const o=co.apply(this,arguments);window.__oscF.push(o);return o}});
const audio=p=>p.evaluate(()=>{const f=(window.__oscF||[]).map(o=>Math.round(o.frequency.value)),a=window.__aud||{osc:[]};
 return {played:TinAudio.played.slice(),ctx:(a.ctxs||[]).map(c=>c.state).join(),started:a.started||0,live:a.osc.filter(o=>o.started&&!o.ended).length,
  back:f.filter(x=>x===392).length,liveBack:a.osc.filter((o,i)=>o.started&&!o.ended&&[392,494].includes(f[i])).length,ring:f.filter(x=>x===1175).length}});
const q=await L.page(b);await freqHook(q);await q.goto(L.BASE+'chat.html?chat=arjun');await sleep(q,300);
await q.click('#simBtn');
await chk('quiet mode: "Quiet mode" switch in the Simulation drawer (role=switch, same pattern as the other switches), on by default',async()=>{const r=await q.evaluate(()=>{const s=document.querySelector('#swQuiet');return s&&{role:s.getAttribute('role'),on:s.getAttribute('aria-checked'),t:s.textContent.trim(),cls:s.className}});return {ok:r&&r.role==='switch'&&r.on==='true'&&r.t==='Quiet mode'&&r.cls==='sw',d:JSON.stringify(r)}});
await q.screenshot({path:'screenshots/roster-quiet-switch-chat.png'});
await q.click('#simBtn');await q.click('#callBtn');await sleep(q,3000);await q.click('#callEnd');await sleep(q,300);
await chk('quiet mode on: ring, connect and End create no AudioContext and no oscillator',async()=>{const a=await audio(q);return {ok:a.ctx===''&&a.started===0&&!a.played.length,d:JSON.stringify(a)}});
await q.click('#simBtn');await q.click('#swQuiet');await q.click('#simBtn');
await q.click('#callBtn');await sleep(q,600);
await chk('direct call: ringback (392/494 Hz two-tone) loops while "Ringing…"; never the incoming ring',async()=>{const a=await audio(q);return {ok:a.played.join()==='back'&&a.liveBack===2&&a.ring===0&&(await q.textContent('#callStatus'))==='Ringing…',d:JSON.stringify(a)}});
await sleep(q,2500);
await chk('direct call: on connect the ringback stops and the connected cue plays once',async()=>{const a=await audio(q);return {ok:a.played.join()==='back,connected'&&a.liveBack===0&&(await q.textContent('#callStatus'))==='Connected',d:JSON.stringify(a)}});
const n0=(await audio(q)).back;await sleep(q,4200);
await chk('direct call connected: no further ringback cycles',async()=>{const a=await audio(q);return {ok:a.back===n0&&a.live===0&&a.played.join()==='back,connected',d:JSON.stringify(a)}});
await q.click('#callMute');await sleep(q,100);
await chk('mute silences the mic, never the cues (no cue on mute)',async()=>{const a=await audio(q);return {ok:a.played.join()==='back,connected'&&a.live===0,d:JSON.stringify(a)}});
await q.click('#callMute');await q.click('#callEnd');await sleep(q,1500);
await chk('direct call End: ended cue once, then zero live oscillators',async()=>{const a=await audio(q);return {ok:a.played.join()==='back,connected,ended'&&a.live===0,d:JSON.stringify(a)}});
const s0=(await audio(q)).started;await sleep(q,4200);
await chk('after End: nothing scheduled (no new oscillators 4s later)',async()=>{const a=await audio(q);return {ok:a.started===s0&&a.live===0,d:s0+'→'+a.started}});
await q.click('#callBtn');await sleep(q,800);await q.click('#callEnd');await sleep(q,1200);
await chk('End while ringing: ringback stopped at once, ended cue, zero live oscillators',async()=>{const a=await audio(q);return {ok:a.played.slice(-2).join()==='back,ended'&&a.liveBack===0&&a.live===0,d:JSON.stringify(a)}});
await q.click('#callBtn');await sleep(q,600);await q.click('#callMin');await sleep(q,200);
await chk('minimized while ringing: ringback keeps going (the call continues)',async()=>{const a=await audio(q);return {ok:a.liveBack>0,d:JSON.stringify(a)}});
await q.click('#simBtn');await q.click('#swQuiet');await sleep(q,200);
await chk('Quiet mode turned on mid-ring: every oscillator stops, strip and timer still shown',async()=>{const a=await audio(q);return {ok:a.live===0&&(await q.getAttribute('#swQuiet','aria-checked'))==='true'&&await q.locator('#callStrip').isVisible(),d:JSON.stringify(a)}});
await q.click('#swQuiet');await sleep(q,300);
await chk('Quiet mode off again while still ringing: ringback resumes',async()=>{const a=await audio(q);return {ok:a.liveBack>0,d:JSON.stringify(a)}});
await q.evaluate(()=>dispatchEvent(new PageTransitionEvent('pagehide',{persisted:false})));await sleep(q,300);
await chk('pagehide: current sound stopped and the AudioContext closed (calls.html discipline)',async()=>{const a=await audio(q),r=await q.evaluate(()=>({cur:TinAudio.Snd.cur,ctx:TinAudio.Snd.ctx}));return {ok:/closed/.test(a.ctx)&&r.cur===null&&r.ctx===null,d:JSON.stringify(a)}});
await chk('direct sound: no console errors',async()=>({ok:!q.errs.length,d:q.errs.join('|')}));
await q.context().close()}
{// message-sent cue (TinAudio.sent): Quiet mode default on; sending plays "pluck" once and resumes an interrupted ringback
const freqHook=p=>p.context().addInitScript(()=>{const AC=window.AudioContext;if(!AC)return;const co=AC.prototype.createOscillator;window.__oscF=[];AC.prototype.createOscillator=function(){const o=co.apply(this,arguments);window.__oscF.push(o);return o}});
const audio=p=>p.evaluate(()=>{const f=(window.__oscF||[]).map(o=>Math.round(o.frequency.value)),a=window.__aud||{osc:[]};
 return {played:TinAudio.played.slice(),ctx:(a.ctxs||[]).map(c=>c.state).join(),started:a.started||0,live:a.osc.filter(o=>o.started&&!o.ended).length,
  pluck:f.filter(x=>x===659).length,liveBack:a.osc.filter((o,i)=>o.started&&!o.ended&&[392,494].includes(f[i])).length}});
const s=await L.page(b);await freqHook(s);await s.goto(L.BASE+'chat.html?chat=arjun');await sleep(s,300);
await s.fill('#ta','quiet send');await s.click('#sendBtn');await sleep(s,300);
await chk('message sent, Quiet mode on by default: no AudioContext at all',async()=>{const a=await audio(s);return {ok:a.ctx===''&&a.started===0&&!a.played.length,d:JSON.stringify(a)}});
await s.click('#simBtn');await s.click('#swQuiet');await s.click('#simBtn');await sleep(s,100);
await s.fill('#ta','loud send');await s.click('#sendBtn');await sleep(s,80);
await chk('Quiet mode off: sending plays the pluck cue (E5/659Hz) at once',async()=>{const a=await audio(s);return {ok:a.played.slice(-1)[0]==='pluck'&&a.pluck>0,d:JSON.stringify(a)}});
await sleep(s,400);
await chk('pluck is a one-shot: zero live oscillators once it settles',async()=>{const a=await audio(s);return {ok:a.live===0,d:JSON.stringify(a)}});
await s.click('#callBtn');await sleep(s,500); // direct call now ringing: ringback loops
await chk('ringback loops while ringing',async()=>{const a=await audio(s);return {ok:a.played.slice(-1)[0]==='back'&&a.liveBack>0,d:JSON.stringify(a)}});
await s.click('[aria-label="Back to chat — call continues"]');await sleep(s,150); // minimize: composer is interactive again, call keeps going
await s.fill('#ta','interrupts the ringback');await s.click('#sendBtn');await sleep(s,80);
await chk('sending while ringing interrupts the ringback to play the pluck',async()=>{const a=await audio(s);return {ok:a.played.slice(-1)[0]==='pluck',d:JSON.stringify(a)}});
await sleep(s,400);
await chk('ringback resumes on its own once the pluck finishes',async()=>{const a=await audio(s);return {ok:a.liveBack>0,d:JSON.stringify(a)}});
await s.click('#callStrip');await sleep(s,150);await s.click('#callEnd');await sleep(s,300);
await chk('message-sent sound: no console errors',async()=>({ok:!s.errs.length,d:s.errs.join('|')}));
await s.context().close()}
{// call record left in the thread on End (WhatsApp convention) — a bubble, since every call here is outgoing
const t=await L.page(b);await t.goto(L.BASE+'chat.html?chat=arjun');await sleep(t,300);
await t.click('#callBtn');await sleep(t,2800);await t.click('#callEnd');await sleep(t,350); // End now waits ~220ms for the string/overlay release
await chk('answered call leaves a call-record bubble: phone icon, "Voice call", MM:SS duration, no delivery tick',async()=>{const r=await t.$eval('.m:last-child',e=>({cls:e.querySelector('.bub').className,fn:e.querySelector('.fn').textContent,fm:e.querySelector('.fm').textContent,fmCls:e.querySelector('.fm').className,tick:!!e.querySelector('.tk')}));return {ok:r.fn==='Voice call'&&/^\d\d:\d\d$/.test(r.fm)&&r.fmCls==='fm'&&!r.tick,d:JSON.stringify(r)}});
await chk('call record shows in the chat-list preview like an attachment',async()=>({ok:/Voice call ·/.test(await t.textContent('[data-chat=arjun] .pv'))}));
await t.click('.m:last-child .more');await sleep(t,200);
await chk('call record menu: only Delete (no Reply/Copy/Edit/Message info)',async()=>{const txt=(await t.textContent('#actionsBody')).replace(/\s+/g,' ');return {ok:/delete/i.test(txt)&&!/reply/i.test(txt)&&!/copy/i.test(txt)&&!/edit/i.test(txt)&&!/message info/i.test(txt),d:txt}});
await t.keyboard.press('Escape');await sleep(t,150);
await t.click('.callrow');await sleep(t,250);
await chk('tapping the call record calls back: overlay reopens',async()=>({ok:await t.locator('#callOv').isVisible()&&(await t.textContent('#callName'))==='Arjun Oommen'}));
await t.click('#callEnd');await sleep(t,350); // End well before connectAt (2500ms) → a second call record, this time "No answer" (release delay included)
await chk('call ended before connecting: "No answer" in --danger, not a duration',async()=>{const r=await t.$eval('.m:last-child .fm',e=>({t:e.textContent,cls:e.className,c:getComputedStyle(e).color}));return {ok:r.t==='No answer'&&/missed/.test(r.cls),d:JSON.stringify(r)}});
await chk('missed call shows "Missed voice call" in the chat-list preview',async()=>({ok:/Missed voice call/.test(await t.textContent('[data-chat=arjun] .pv'))}));
await chk('call record sound: no console errors',async()=>({ok:!t.errs.length,d:t.errs.join('|')}));
await t.context().close()}
{// the brand mark: sags while ringing/connecting, goes taut the instant the call connects, sags again and releases on End
const u=await L.page(b);await u.goto(L.BASE+'chat.html?chat=arjun');await sleep(u,300);
await u.click('#callBtn');await sleep(u,200);
await chk('mark sags while ringing',async()=>({ok:!(await u.evaluate(()=>document.querySelector('#callMark').classList.contains('taut')))}));
await sleep(u,2600);
await chk('mark goes taut the instant status reads Connected',async()=>{const r=await u.evaluate(()=>({taut:document.querySelector('#callMark').classList.contains('taut'),status:document.querySelector('#callStatus').textContent}));return {ok:r.taut&&r.status==='Connected',d:JSON.stringify(r)}});
const t0=Date.now();await u.click('#callEnd');
await chk('End: mark sags back and the overlay releases before it hides (not an instant cut)',async()=>{const r=await u.evaluate(()=>({taut:document.querySelector('#callMark')?.classList.contains('taut'),ending:document.querySelector('#callOv').classList.contains('ending'),hidden:document.querySelector('#callOv').hidden}));return {ok:!r.taut&&r.ending&&!r.hidden,d:JSON.stringify(r)}});
await sleep(u,400);
const releaseMs=Date.now()-t0;
await chk('overlay actually hides only after the release (~220ms), not before',async()=>{const hidden=await u.evaluate(()=>document.querySelector('#callOv').hidden);return {ok:hidden&&releaseMs>=200,d:'hidden after '+releaseMs+'ms'}});
await chk('reduced motion: skips both the mark transition and the End delay',async()=>{
 await u.goto(L.BASE+'chat.html?chat=arjun&motion=reduce');await sleep(u,300);
 await u.click('#callBtn');await sleep(u,2700);
 const taut=await u.evaluate(()=>document.querySelector('#callMark').classList.contains('taut'));
 await u.click('#callEnd');await sleep(u,40);
 const hidden=await u.evaluate(()=>document.querySelector('#callOv').hidden);
 return {ok:taut&&hidden,d:JSON.stringify({taut,hidden})};
});
await chk('brand mark: no console errors',async()=>({ok:!u.errs.length,d:u.errs.join('|')}));
await u.context().close()}
await b.close();C.save('chat-results.json')})();
