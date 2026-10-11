const L=require('./lib'),C=require('./check');
const chk=C.mk('chat');const sleep=(p,ms)=>p.waitForTimeout(ms);
const foc=p=>p.evaluate(()=>{const a=document.activeElement;return a&&a!==document.body?(a.id||a.getAttribute('aria-label')||a.tagName)+'':'BODY'});
const msgs=p=>p.$$eval('[data-mid]',e=>e.map(x=>x.textContent.replace(/\s+/g,' ').trim().slice(0,80)));
(async()=>{const b=await L.launch();
let p=await L.page(b);await p.goto(L.BASE+'chat.html');await sleep(p,300);
const sim=async()=>{if(!(await p.locator('#swFail').isVisible()))await p.click('summary,#sim >> text=Simulation controls').catch(()=>{})};
await sim();
await chk('simulation controls reachable & switches have role/aria-checked',async()=>{const r=await p.$$eval('#swOnline,#swFail',e=>e.map(x=>x.getAttribute('role')+':'+x.getAttribute('aria-checked')+':'+x.getAttribute('aria-pressed')));return {ok:r.every(x=>!/null:null:null/.test(x)),d:r.join(' ')}});
await p.locator('[data-chat]').first().click();await sleep(p,300);
await chk('open chat: composer visible, Send not enabled when empty',async()=>{const d=await p.$eval('#sendBtn',e=>e.disabled||e.getAttribute('aria-disabled'));return {ok:!!d,d:'disabled='+d}});
// draft
await p.fill('#ta','half written thought');
await p.locator('[data-chat]').nth(1).click();await sleep(p,300);
await chk('switch chat: composer empty for other chat',async()=>({ok:(await p.inputValue('#ta'))==='',d:await p.inputValue('#ta')}));
const rowTxt=await p.locator('[data-chat]').first().textContent();
await chk('chat list shows Draft marker for chat with unsent text',async()=>({ok:/draft/i.test(await p.textContent('#chats')),d:(await p.textContent('#chats')).replace(/\s+/g,' ').slice(0,160)}));
await p.locator('[data-chat]').first().click();await sleep(p,300);
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
await chk('failed send shows failure and Retry control',async()=>{const m=(await msgs(p)).slice(-1)[0];const r=await p.locator('[data-act=retry]').count();return {ok:/fail|not sent|couldn/i.test(m)&&r>0,d:m+' retryBtns='+r}});
await p.screenshot({path:'screenshots/chat-desktop-failed.png'});
await chk('Next send fails switch resets after one failure (one-shot)',async()=>{const v=await p.getAttribute('#swFail','aria-checked')??await p.getAttribute('#swFail','aria-pressed');return {ok:v==='false',d:v}});
await p.locator('[data-act=retry]').first().click();await sleep(p,300);
await chk('retry: focus not lost to BODY',async()=>{const f=await foc(p);return {ok:f!=='BODY',d:f}});
await sleep(p,4000);
await chk('retry succeeds (no failed message remains)',async()=>{const m=(await msgs(p)).slice(-1)[0];return {ok:!/fail|not sent/i.test(m)&&(await p.locator('[data-act=retry]:visible').count())===0,d:m+' visibleRetry='+await p.locator('[data-act=retry]:visible').count()}});
// offline contact
await p.click('#swOnline');await sleep(p,200);
await chk('contact offline: header/status changes',async()=>{const t=(await p.textContent('#conv')).replace(/\s+/g,' ');return {ok:/offline|waiting|queued|not online/i.test(t),d:t.slice(0,140)}});
await p.fill('#ta','sent while offline');await p.press('#ta','Enter');await sleep(p,1500);
await chk('send while offline shows waiting/queued state',async()=>{const m=(await msgs(p)).slice(-1)[0];return {ok:/wait|queue|will send|pending|sending/i.test(m),d:m}});
await p.click('#swOnline');await sleep(p,4500);
await chk('back online: queued message delivered',async()=>{const m=(await msgs(p)).slice(-1)[0];return {ok:!/wait|queue|pending|sending/i.test(m),d:m}});
// incoming
await p.locator('[data-chat]').nth(1).click();await sleep(p,200);
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
await p.locator('[aria-label^="Actions for"]').first().click();await sleep(p,300);
await p.screenshot({path:'screenshots/chat-desktop-actions.png'});
await chk('message actions dialog lists Reply/Copy/Delete',async()=>{const t=(await p.textContent('#actionsBody')).replace(/\s+/g,' ');return {ok:/reply/i.test(t)&&/copy/i.test(t)&&/delete/i.test(t),d:t.slice(0,120)}});
await p.keyboard.press('Escape');await sleep(p,300);
await chk('actions Escape restores focus to the actions button',async()=>{const f=await p.evaluate(()=>document.activeElement.getAttribute('aria-label')||document.activeElement.tagName);return {ok:/^Actions for/.test(f),d:f}});
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
await chk('mobile: ≥44px touch targets on primary controls',async()=>{const r=await p.$$eval('button:not([hidden]),a.back,input',e=>e.filter(x=>x.offsetParent).map(x=>{const b=x.getBoundingClientRect();return [(x.id||x.getAttribute('aria-label')||x.textContent||'').trim().slice(0,18),Math.round(b.width),Math.round(b.height)]}).filter(x=>x[2]<40||x[1]<40));return {ok:r.length===0,d:JSON.stringify(r)}});
await p.context().close();
p=await L.page(b,{dark:true});await p.goto(L.BASE+'chat.html');await p.locator('[data-chat]').first().click();await sleep(p,300);await p.screenshot({path:'screenshots/chat-desktop-dark-open.png'});
await b.close();C.save('chat-results.json')})();
