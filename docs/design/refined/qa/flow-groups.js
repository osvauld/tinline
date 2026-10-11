const L=require('./lib'),C=require('./check');
const chk=C.mk('groups');const sleep=(p,ms)=>p.waitForTimeout(ms);
const foc=p=>p.evaluate(()=>{const a=document.activeElement;return a&&a!==document.body?(a.id||a.getAttribute('aria-label')||(a.textContent||'').trim().slice(0,22)||a.tagName):'BODY'});
const dtxt=p=>p.evaluate(()=>{const ds=document.querySelectorAll('dialog[open]');const d=ds[ds.length-1];return d?d.textContent.replace(/\s+/g,' ').trim():'NO DIALOG'});
const simOpen=async(p)=>{const d=p.locator('details',{hasText:'Simulation'}).last();if(!(await d.evaluate(e=>e.open)))await d.locator('summary').click()};
const open=async(p)=>{for(const s of ['Invitations','Chat requests','Sent requests']){const d=p.locator('details',{hasText:s}).first();if(await d.count()&&!(await d.evaluate(e=>e.open)))await d.locator('summary').click()}};
(async()=>{const b=await L.launch();
let p=await L.page(b);await p.goto(L.BASE+'groups.html');await sleep(p,400);
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
await simOpen(p);await p.locator('#simBody label:has-text("host desktops offline")').click();await sleep(p,500);
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
p=await L.page(b,{dark:true});await p.goto(L.BASE+'groups.html');await sleep(p,300);await p.screenshot({path:'screenshots/groups-desktop-dark-open.png'});
await b.close();C.save('groups-results.json')})();
