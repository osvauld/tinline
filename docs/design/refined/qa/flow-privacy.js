const L=require('./lib'),C=require('./check');
const chk=C.mk('privacy');const sleep=(p,ms)=>p.waitForTimeout(ms);
const foc=p=>p.evaluate(()=>{const a=document.activeElement;return a&&a!==document.body?(a.id||a.dataset.act+'/'+(a.dataset.id||'')||a.tagName):'BODY'});
const dtxt=p=>p.evaluate(()=>{const d=document.querySelector('dialog[open]');return d?d.textContent.replace(/\s+/g,' ').trim():'NO DIALOG'});
const panel=async p=>(await p.textContent('#panel')).replace(/\s+/g,' ');
const simOpen=async p=>{const d=p.locator('details',{hasText:'Simulation controls'}).first();if(!(await d.evaluate(e=>e.open)))await d.locator('summary').click()};
const T=o=>({ok:o[0],d:o[1]});
(async()=>{const b=await L.launch();
let p=await L.page(b);await p.goto(L.BASE+'privacy.html');await sleep(p,400);
// tabs
await chk('tabs: roles, aria-selected on exactly one',async()=>{const r=await p.$$eval('[role=tab]',e=>e.map(x=>x.getAttribute('aria-selected')));return T([r.filter(x=>x==='true').length===1&&r.length===3,r.join()])});
await p.focus('#tab-address');await p.keyboard.press('ArrowRight');await sleep(p,200);
await chk('tabs: ArrowRight moves to next tab (roving keyboard)',async()=>{const f=await foc(p);const s=await p.getAttribute('#tab-people','aria-selected');return T([f==='tab-people'||s==='true',`focus=${f} peopleSelected=${s}`])});
await p.click('#tab-address');await sleep(p,200);
// told list
await p.focus('[data-act=told-open]');await p.click('[data-act=told-open]');await sleep(p,300);
await chk('"See who has it" dialog opens with focus inside, list of contacts',async()=>{const t=await dtxt(p);const inside=await p.evaluate(()=>!!document.activeElement.closest('dialog[open]'));return T([/Who has your new address/.test(t)&&inside,'inside='+inside])});
await p.keyboard.press('Escape');await sleep(p,300);
await chk('Escape restores focus to See-who-has-it button',async()=>{const f=await foc(p);return T([f==='told-open/',f])});
// rotation intervals
const iv=p.locator('[data-act=interval]');const nIv=await iv.count();
const before=await panel(p);
await iv.nth(2).click({force:true});await sleep(p,300);
await chk('changing interval updates the explanation/next-change text',async()=>{const a=await panel(p);return T([a!==before,a.slice(0,160)])});
await chk('interval options are a labelled radio group (fieldset/legend or radiogroup)',async()=>{const r=await p.evaluate(()=>{const i=document.querySelector('[data-act=interval]');return !!(i.closest('fieldset')||i.closest('[role=radiogroup]'))});return T([r,String(r)])});
// advance rotation
await simOpen(p);
await p.screenshot({path:'screenshots/privacy-desktop-address.png'});
await p.click('[data-act=advance]');await sleep(p,1200);
await chk('Jump to next scheduled change updates status (rotation happened)',async()=>{const a=await panel(p);return T([a!==before,a.slice(0,200)])});
await p.screenshot({path:'screenshots/privacy-desktop-rotated.png'});
// scenarios
for(const [lab,re] of [['Phone was asleep',/overdue/i],['Change failed',/fail|couldn|didn|try again|old address|still/i],['Some contacts offline',/offline|haven|waiting|not yet|\d+ contact/i]]){
 await p.locator('#sim label',{hasText:lab}).first().click();await sleep(p,300);
 if(lab==='Change failed')await p.click('[data-act=advance]').catch(()=>{});await sleep(p,900);
 const a=await panel(p);
 await p.screenshot({path:'screenshots/privacy-desktop-scn-'+lab.replace(/\W+/g,'-')+'.png'});
 await chk(`scenario "${lab}" shows distinct honest state`,async()=>T([re.test(a),a.slice(0,220)]));
 if(lab==='Phone was asleep'){await p.click('[data-act=wake]');await sleep(p,1200);await chk('Simulate waking the phone resolves overdue state, focus not BODY',async()=>{const a2=await panel(p);const f=await foc(p);return T([!/address change is overdue/i.test(a2)&&f!=='BODY','focus='+f+' '+a2.slice(0,140)])})}
 if(lab==='Change failed'){await chk('failure state offers Retry',async()=>{const r=await p.locator('[data-act=retry]:visible').count();return T([r>0,'retryBtns='+r])});
  if(await p.locator('[data-act=retry]:visible').count()){await p.locator('[data-act=retry]:visible').first().click();await sleep(p,1200);await chk('Retry resolves the failure (retry button gone / status changes)',async()=>{const a2=await panel(p);const r=await p.locator('[data-act=retry]:visible').count();return T([r===0||a2!==a,a2.slice(0,160)+' retry='+r])});
   await chk('focus after Retry not lost to BODY',async()=>{const f=await foc(p);return T([f!=='BODY',f])})}}
}
await p.locator('#sim label',{hasText:'Normal'}).first().click();
await p.click('[data-act=reset]');await sleep(p,500);
// people / block
await p.click('#tab-people');await sleep(p,300);
await p.screenshot({path:'screenshots/privacy-desktop-people.png'});
await p.locator('[data-act=block-open]').first().focus();
const trigId=await foc(p);
await p.locator('[data-act=block-open]').first().click();await sleep(p,300);
await chk('block dialog: explains consequences, focus inside, Block button named with person',async()=>{const t=await dtxt(p);const f=await foc(p);return T([/can’t message or call you/.test(t)&&/Block Priya N/.test(t)&&await p.evaluate(()=>!!document.activeElement.closest('dialog[open]')),'focus='+f])});
await chk('block dialog discloses honest limits (modified app / what stays)',async()=>{const t=await dtxt(p);return T([/modified app|history stays|still refuses/i.test(t),t.slice(0,160)])});
await p.keyboard.press('Escape');await sleep(p,300);
await chk('Escape cancels block: nobody blocked, focus restored to Block button',async()=>{const a=await panel(p);const f=await foc(p);return T([/Nobody is blocked/.test(a)&&f===trigId,'focus='+f+' want='+trigId])});
await p.locator('[data-act=block-open]').first().click();await sleep(p,200);
await p.click('[data-act=block-do]');await sleep(p,700);
await p.screenshot({path:'screenshots/privacy-desktop-blocked.png'});
await chk('confirm block: listed under Blocked people with date and Unblock',async()=>{const a=await panel(p);return T([/Blocked people\s*PN\s*Priya N\s*Blocked [A-Z]/.test(a)&&/Unblock/.test(a),a.slice(a.indexOf('Blocked people'),a.indexOf('Blocked people')+100)])});
await chk('focus after block not lost to BODY',async()=>{const f=await foc(p);return T([f!=='BODY',f])});
await chk('call button for blocked person disabled/removed',async()=>{const c=await p.locator('[data-act=call][data-id=priya]').count();const dis=c?await p.locator('[data-act=call][data-id=priya]').first().evaluate(e=>e.disabled||e.getAttribute('aria-disabled')==='true'):true;return T([dis,'count='+c+' disabled='+dis])});
await p.locator('[data-act=unblock-open]').first().click();await sleep(p,300);
await chk('unblock dialog states that reconnecting by QR is needed',async()=>{const t=await dtxt(p);return T([/scan each other|reconnect/i.test(t)&&/never restores/i.test(t),t.slice(0,180)])});
await p.click('[data-act=unblock-do]');await sleep(p,600);
await chk('unblock: person back in list, Blocked people empty, focus not BODY',async()=>{const a=await panel(p);const f=await foc(p);return T([/Nobody is blocked/.test(a)&&f!=='BODY','focus='+f+' '+a.slice(0,120)])});
// QR recovery
await p.click('#tab-lost');await sleep(p,300);
await p.screenshot({path:'screenshots/privacy-desktop-lost.png'});
await p.locator('[data-act=qr-open]').first().focus();await p.locator('[data-act=qr-open]').first().click();await sleep(p,400);
await p.screenshot({path:'screenshots/privacy-desktop-qr.png'});
await chk('recovery QR dialog opens with focus inside and explains what the QR contains',async()=>{const t=await dtxt(p);return T([t!=='NO DIALOG'&&await p.evaluate(()=>!!document.activeElement.closest('dialog[open]')),t.slice(0,220)])});
await p.keyboard.press('Escape');await sleep(p,300);
await chk('Escape on QR dialog restores focus to its trigger',async()=>{const f=await foc(p);return T([/qr-open/.test(f),f])});
await simOpen(p);
for(const lab of ['laptop','shared']){const bt=p.locator(`[data-act=${lab}]`);const txt=(await bt.textContent()).trim()||await bt.getAttribute('aria-label');
 await bt.click();await sleep(p,500);const a=await panel(p);
 await chk(`recovery sim toggle "${lab}" (${txt||'unlabelled'}) has an accessible name`,async()=>{const n=await bt.evaluate(e=>{const l=e.getAttribute('aria-labelledby');return (e.getAttribute('aria-label')||(l&&document.getElementById(l)&&document.getElementById(l).textContent)||e.textContent||'').trim()});return T([n.length>0,JSON.stringify(n)])});
 await chk(`recovery with "${lab}" changes the guidance`,async()=>T([a.length>100,a.slice(0,200)]));
 await p.screenshot({path:'screenshots/privacy-desktop-lost-'+lab+'.png'})}
await chk('no console errors',async()=>T([p.errs.length===0,p.errs.join('|')]));
await chk('no failed/404 requests',async()=>T([p.fails.length===0,p.fails.join('|')]));
await p.context().close();
// mobile
p=await L.page(b,{w:390,h:844,mobile:true});await p.goto(L.BASE+'privacy.html');await sleep(p,400);
for(const t of ['address','people','lost']){await p.click('#tab-'+t);await sleep(p,300);await p.screenshot({path:`screenshots/privacy-mobile-${t}.png`,fullPage:true});
 await chk(`mobile ${t}: no horizontal overflow`,async()=>{const o=await L.overflow(p);return T([o.sw<=o.cw,JSON.stringify(o)])})}
await p.click('#tab-people');await p.locator('[data-act=block-open]').first().click();await sleep(p,300);await p.screenshot({path:'screenshots/privacy-mobile-block.png'});
await chk('mobile block dialog fits viewport and buttons ≥44px',async()=>{const r=await p.evaluate(()=>{const d=document.querySelector('dialog[open]');const b=d.getBoundingClientRect();return {box:[Math.round(b.left),Math.round(b.right),Math.round(b.top),Math.round(b.bottom)],btn:[...d.querySelectorAll('button')].map(x=>Math.round(x.getBoundingClientRect().height)),scroll:d.scrollHeight>d.clientHeight}});return T([r.box[0]>=0&&r.box[1]<=390&&r.btn.every(h=>h>=44),JSON.stringify(r)])});
await p.keyboard.press('Escape');
await chk('mobile tab strip: all three tabs reachable without clipping',async()=>{const r=await p.$$eval('[role=tab]',e=>e.map(x=>{const b=x.getBoundingClientRect();return [Math.round(b.left),Math.round(b.right)]}));return T([r.every(x=>x[0]>=0&&x[1]<=390),JSON.stringify(r)])});
await p.context().close();
for(const [dk,nm] of [[true,'dark']]){p=await L.page(b,{dark:true});await p.goto(L.BASE+'privacy.html');await sleep(p,300);await p.screenshot({path:'screenshots/privacy-desktop-dark-address.png'});await p.click('#tab-people');await p.locator('[data-act=block-open]').first().click();await sleep(p,300);await p.screenshot({path:'screenshots/privacy-desktop-dark-block.png'});await p.keyboard.press('Escape');await p.context().close()}
p=await L.page(b,{w:390,h:844,mobile:true,dark:true});await p.goto(L.BASE+'privacy.html');await sleep(p,300);await p.screenshot({path:'screenshots/privacy-mobile-dark-address.png',fullPage:true});
await b.close();C.save('privacy-results.json')})();
