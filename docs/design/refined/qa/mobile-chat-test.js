const {launch,page,BASE}=require('./lib.js');const fs=require('fs'),path=require('path');
const SS=path.join(__dirname,'screenshots'),results=[];
const chk=(n,ok,d='')=>{results.push({name:n,ok:!!ok,detail:String(d)});if(!ok)console.log('FAIL',n,d)};
const shot=(p,n)=>p.screenshot({path:path.join(SS,'mobile-chat-'+n+'.png')});
const ovf=p=>p.evaluate(()=>({sw:document.documentElement.scrollWidth,iw:innerWidth,bw:document.body.scrollWidth}));
async function step(n,f){try{await f()}catch(e){chk(n+' (exception)',false,e.message.split('\n')[0])}}
(async()=>{const b=await launch();const _c=require('./lib.js');
// 1. Desktop 1440 with explicit mobile preview
await step('desktop-mobile',async()=>{const p=await page(b);await p.goto(BASE+'mobile.html?chat=arjun');await p.waitForTimeout(700);
 const f=p.frameLocator('#phone');
 chk('1440 mobile.html iframe src deep-links chat',(await p.locator('#phone').getAttribute('src')).includes('chat=arjun'));
 chk('1440 iframe conv view',await f.locator('#app[data-view=conv]').count()===1);
 chk('1440 iframe composer visible',await f.locator('#ta').isVisible());
 chk('1440 iframe uses embed=1',(await p.locator('#phone').getAttribute('src')).includes('embed=1'));
 chk('1440 embed: no in-frame back, no harness chrome',!(await f.locator('.back-list').isVisible())&&!(await f.locator('.harness').isVisible()));
 chk('1440 embed: call button present (44px)',await f.locator('#callBtn').evaluate(e=>e.tagName==='BUTTON'&&e.getBoundingClientRect().width>=44));
 await f.locator('#callBtn').click();await p.waitForTimeout(300);
 chk('1440 embed: call overlay fills the iframe, outer page unchanged',await f.locator('#callOv').evaluate(e=>{const r=e.getBoundingClientRect();return Math.abs(r.width-innerWidth)<2&&Math.abs(r.height-innerHeight)<2})&&p.url().includes('mobile.html'));
 await shot(p,'1440-iframe-call');
 await f.locator('#callMin').click();await p.waitForTimeout(200);
 chk('1440 embed: minimized strip above header inside the frame',await f.locator('#callStrip').evaluate(e=>{const r=e.getBoundingClientRect(),b=document.querySelector('#conv .bar').getBoundingClientRect();return r.top<2&&Math.round(r.height)===40&&b.top>=r.bottom-1}));
 await shot(p,'1440-iframe-strip');
 await f.locator('#callStrip').click();await f.locator('#callEnd').click();await p.waitForTimeout(200);
 chk('1440 embed: call ended',!(await f.locator('#callStrip').isVisible())&&!(await f.locator('#callOv').isVisible()));
 chk('1440 embed: app fills frame, no border/radius',await f.locator('#app').evaluate(e=>{const c=getComputedStyle(e),r=e.getBoundingClientRect();return c.borderTopWidth==='0px'&&c.borderTopLeftRadius==='0px'&&Math.abs(r.height-innerHeight)<2}));
 chk('1440 iframe shows Arjun',(await f.locator('#convTitle').innerText()).includes('Arjun'));
 chk('1440 iframe width 390',await p.locator('#phone').evaluate(e=>e.clientWidth)===390);
 chk('1440 outer no overflow',(o=>o.sw<=o.iw)(await ovf(p)));
 const fr=p.frames().find(x=>x.url().includes('chat.html'));
 chk('1440 iframe inner no overflow',(o=>o.sw<=o.iw)(await ovf(fr)),JSON.stringify(await ovf(fr)));
 const r=await p.locator('#phone').boundingBox();chk('1440 phone fits in viewport',r.y+r.height<=1000,JSON.stringify(r));
 const cb=await f.locator('.composer').boundingBox();chk('1440 composer inside iframe',cb.y+cb.height<=r.y+r.height+1,JSON.stringify(cb));
 await shot(p,'1440-iframe');
 // width toggle
 await p.selectOption('#width','320');await p.waitForTimeout(300);
 chk('toggle 320 iframe width',await p.locator('#phone').evaluate(e=>e.clientWidth)===320);
 chk('320 iframe inner no overflow',(o=>o.sw<=o.iw)(await ovf(p.frames().find(x=>x.url().includes('chat.html')))));
 chk('320 iframe composer visible',await f.locator('#ta').isVisible());
 await shot(p,'1440-iframe-320');
 await p.selectOption('#width','390');await p.waitForTimeout(200);chk('toggle back to 390',await p.locator('#phone').evaluate(e=>e.clientWidth)===390);
 // desktop link
 const href=await p.locator('#desktop').getAttribute('href');chk('desktop link href',href==='chat.html?chat=arjun',href);
 await p.locator('#desktop').click();await p.waitForTimeout(500);chk('desktop link opens desktop chat (split)',p.url().includes('chat.html')&&await p.locator('.list-pane').isVisible()&&await p.locator('#ta').isVisible());await shot(p,'desktop-link');
 // other chat param
 await p.goto(BASE+'mobile.html?chat=meera');await p.waitForTimeout(500);chk('mobile.html chat=meera selects Meera',(await p.frameLocator('#phone').locator('#convTitle').innerText()).includes('Meera'));
 // invalid
 for(const bad of ['nobody','<script>alert(1)</script>','','__proto__']){const q=await page(b);await q.goto(BASE+'mobile.html?chat='+encodeURIComponent(bad));await q.waitForTimeout(500);
  const t=await q.frameLocator('#phone').locator('#convTitle').innerText().catch(()=> '(none)');chk('mobile.html invalid chat "'+bad+'" safe fallback',t.includes('Arjun')&&!q.errs.length,t+' '+q.errs.join('|'));await q.close()}
 const q=await page(b,{w:390,h:844});await q.goto(BASE+'chat.html?chat=nobody');await q.waitForTimeout(500);
 chk('chat.html invalid param shows list, no errors',await q.locator('#app[data-view=list]').count()===1&&!q.errs.length,q.errs.join('|'));await shot(q,'invalid-chat-param');
 chk('chat.html invalid param no overflow',(o=>o.sw<=o.iw)(await ovf(q)));await q.close();
 chk('1440 no runtime errors',!p.errs.length,p.errs.join('|'));await p.close()});
// 2. native 390 / 320
for(const w of [390,320]){await step('native'+w,async()=>{const h=w===390?844:640;const p=await page(b,{w,h,mobile:true});
 await p.goto(BASE+'chat.html?chat=arjun');await p.waitForTimeout(500);
 chk(w+' native conv view',await p.locator('#app[data-view=conv]').count()===1);
 chk(w+' native selected Arjun',(await p.locator('#convTitle').innerText()).includes('Arjun'));
 chk(w+' no overflow',(o=>o.sw<=o.iw)(await ovf(p)),JSON.stringify(await ovf(p)));
 const cb=await p.locator('.composer').boundingBox(),bb=await p.locator('.back-list').boundingBox();
 chk(w+' composer within viewport',cb.y>=0&&cb.y+cb.height<=h&&cb.x>=0&&cb.x+cb.width<=w,JSON.stringify(cb));
 chk(w+' back target >=44',bb.width>=44&&bb.height>=44,JSON.stringify(bb));
 for(const sel of ['#sendBtn','[aria-label="Attach a file"]']){const bx=await p.locator(sel).boundingBox();chk(w+' '+sel+' >=44px',bx.width>=44&&bx.height>=44,JSON.stringify(bx))}
 await shot(p,'native-'+w);
 // draft/send
 await p.locator('#ta').fill('Draft for QA');chk(w+' send enabled with draft',await p.locator('#sendBtn').getAttribute('aria-disabled')==='false');
 await p.locator('.back-list').click();await p.waitForTimeout(200);chk(w+' back (no from) shows list',await p.locator('#app[data-view=list]').count()===1);
 chk(w+' draft preview in list',(await p.locator('[data-chat=arjun]').innerText()).includes('Draft'),await p.locator('[data-chat=arjun]').innerText());await shot(p,'native-'+w+'-list-draft');
 await p.locator('[data-chat=arjun]').click();await p.waitForTimeout(200);chk(w+' draft restored',await p.locator('#ta').inputValue()==='Draft for QA');
 const before=await p.locator('#msgs .m').count();await p.locator('#sendBtn').click();await p.waitForTimeout(300);
 chk(w+' send adds message',await p.locator('#msgs .m').count()===before+1);chk(w+' composer cleared',await p.locator('#ta').inputValue()==='');
 const last=await p.locator('#msgs .m').last().boundingBox();chk(w+' new message above composer',last.y+last.height<=(await p.locator('.composer').boundingBox()).y+2,JSON.stringify(last));
 await p.locator('#sendBtn').click({force:true});chk(w+' empty send gives no message',await p.locator('#msgs .m').count()===before+1);
 // long text
 await p.locator('#ta').fill('A '.repeat(200)+'veryveryveryveryveryveryveryveryveryveryveryveryveryverylongwordwithoutspaces'.repeat(2));await p.waitForTimeout(150);
 const cb2=await p.locator('.composer').boundingBox();chk(w+' long draft composer stays in viewport',cb2.y>=0&&cb2.y+cb2.height<=h+1,JSON.stringify(cb2));chk(w+' long draft no overflow',(o=>o.sw<=o.iw)(await ovf(p)));await shot(p,'native-'+w+'-longdraft');
 await p.locator('#ta').fill('');
 // attach
 await p.locator('[aria-label="Attach a file"]').click();await p.waitForTimeout(250);
 const d=await p.locator('#dAttach').boundingBox();chk(w+' attach sheet within viewport',d&&d.x>=-1&&d.x+d.width<=w+1&&d.y>=-1&&d.y+d.height<=h+1,JSON.stringify(d));await shot(p,'native-'+w+'-attach');
 const sb=p.locator('#dAttach .list button').first();await sb.click();await p.waitForTimeout(400);
 chk(w+' attachment adds message',await p.locator('#msgs .m').count()===before+2);chk(w+' attach sheet closes',await p.locator('#dAttach[open]').count()===0);
 chk(w+' focus returns sensible after attach',await p.evaluate(()=>!!document.activeElement&&document.activeElement!==document.body),await p.evaluate(()=>document.activeElement.outerHTML.slice(0,80)));await shot(p,'native-'+w+'-attached');
 // message menu
 chk(w+' no visible per-message menu buttons',await p.$$eval('#msgs .more',e=>e.every(x=>x.getBoundingClientRect().width<=1)));
 const tbub=p.locator('#msgs .m.me .bub').first();const tb=await tbub.boundingBox();
 const lp=(dx,ms)=>tbub.evaluate(async(el,[dx,ms])=>{const r=el.getBoundingClientRect(),x=r.left+r.width/2,y=r.top+r.height/2,o={bubbles:true,pointerType:'touch',isPrimary:true,pointerId:7};
  el.dispatchEvent(new PointerEvent('pointerdown',{...o,clientX:x,clientY:y,button:0}));if(dx)el.dispatchEvent(new PointerEvent('pointermove',{...o,clientX:x+dx,clientY:y}));
  await new Promise(r=>setTimeout(r,ms));el.dispatchEvent(new PointerEvent('pointerup',{...o,clientX:x+dx,clientY:y}))},[dx,ms]);
 await lp(14,650);chk(w+' long-press cancelled by >10px move',await p.locator('dialog[open]').count()===0);
 await lp(0,300);chk(w+' short press does not open menu',await p.locator('dialog[open]').count()===0);
 await lp(0,650);await p.waitForTimeout(250);
 const open=await p.locator('#dActions[open]').count();chk(w+' long-press (500ms) opens message sheet',open===1&&(await p.getAttribute('#dActions','class'))==='sheet');
 if(open){const mb=await p.locator('dialog[open]').boundingBox();chk(w+' sheet docked to bottom, within viewport',mb.x>=-1&&mb.x+mb.width<=w+1&&mb.y>=-1&&Math.abs(mb.y+mb.height-h)<=2,JSON.stringify(mb));
  const t=(await p.textContent('#actionsBody')).replace(/\s+/g,' ');chk(w+' sheet items Reply/Copy/Edit/Delete/Message info',/Reply.*Copy.*Edit.*Delete.*Message info/.test(t),t);
  await shot(p,'native-'+w+'-menu');await p.keyboard.press('Escape');await p.waitForTimeout(150);chk(w+' menu Escape closes',await p.locator('dialog[open]').count()===0);
  chk(w+' focus returns to the message',await p.evaluate(()=>!!document.activeElement.closest('.m')))}
 await tbub.dispatchEvent('contextmenu');await p.waitForTimeout(250);chk(w+' contextmenu opens sheet',await p.locator('#dActions[open].sheet').count()===1);
 await p.mouse.click(w/2,40);await p.waitForTimeout(200);chk(w+' scrim tap closes sheet',await p.locator('dialog[open]').count()===0);
 await p.locator('#msgs .m.me .more').first().focus();await p.keyboard.press('Enter');await p.waitForTimeout(250);chk(w+' keyboard: hidden menu button opens sheet',await p.locator('#dActions[open]').count()===1);
 if(await p.locator('#dActions[open]').count()){await p.locator('#dActions .mi',{hasText:'Message info'}).click();await p.waitForTimeout(250);chk(w+' Message info sheet opens',await p.locator('#dMsgInfo[open]').count()===1);await shot(p,'native-'+w+'-msginfo');await p.keyboard.press('Escape');await p.waitForTimeout(150)}
 // info sheet
 await p.locator('#convTitle').click();await p.waitForTimeout(250);const ib=await p.locator('dialog[open]').boundingBox().catch(()=>null);
 chk(w+' details sheet within viewport',ib&&ib.x>=-1&&ib.x+ib.width<=w+1&&ib.y>=-1&&ib.y+ib.height<=h+1,JSON.stringify(ib));await shot(p,'native-'+w+'-info');await p.keyboard.press('Escape');
 // keyboard simulation: shrink viewport
 await p.setViewportSize({width:w,height:Math.round(h*0.5)});await p.waitForTimeout(250);await p.locator('#ta').focus();
 const kc=await p.locator('.composer').boundingBox(),kt=await p.locator('#conv .bar').boundingBox();
 chk(w+' keyboard-height: composer visible',kc.y>=0&&kc.y+kc.height<=h*0.5+1,JSON.stringify(kc));chk(w+' keyboard-height: header visible',kt.y>=0,JSON.stringify(kt));
 const ml=await p.locator('#msgs').boundingBox();chk(w+' keyboard-height: message list has room (>=40px)',ml.height>=40,JSON.stringify(ml));await shot(p,'native-'+w+'-keyboard');
 chk(w+' native no runtime errors',!p.errs.length,p.errs.join('|'));await p.close()})}
// 3. home -> chat -> back (390, 320)
for(const w of [390,320]){await step('home'+w,async()=>{const p=await page(b,{w,h:w===390?844:640,mobile:true});await p.goto(BASE+'home.html');await p.waitForTimeout(900);
 await p.locator('#list [data-person=arjun]').click();await p.waitForTimeout(500);
 chk(w+' home->Arjun lands on chat.html',p.url().includes('chat.html?chat=arjun'),p.url());chk(w+' from=home in URL',p.url().includes('from=home'),p.url());
 chk(w+' no modal open after navigation',await p.locator('dialog[open]').count()===0);
 chk(w+' direct chat conv view',await p.locator('#app[data-view=conv]').count()===1);chk(w+' chat shows Arjun',(await p.locator('#convTitle').innerText()).includes('Arjun'));await shot(p,'home-to-chat-'+w);
 chk(w+' no overflow after nav',(o=>o.sw<=o.iw)(await ovf(p)));
 await p.locator('.back-list').click();await p.waitForTimeout(700);
 chk(w+' Back returns home.html',p.url().includes('home.html'),p.url());chk(w+' Back lands on Chats tab',(await p.locator('#page-title').textContent())==='Chats',await p.locator('#page-title').textContent());
 chk(w+' home list rows after Back',await p.locator('#list [data-person]').count()===6);chk(w+' home no modal after back',await p.locator('dialog[open]').count()===0);
 chk(w+' home launch not blocking after Back',await p.locator('#app').evaluate(e=>!e.inert));await shot(p,'home-back-'+w);
 // browser back history
 await p.locator('#list [data-person=meera]').click();await p.waitForTimeout(400);chk(w+' home->Meera correct person',(await p.locator('#convTitle').innerText()).includes('Meera'));
 await p.goBack();await p.waitForTimeout(600);chk(w+' browser Back returns home',p.url().includes('home.html'),p.url());
 // group row opens the groups study
 await p.locator('#mobile-nav [data-tab=groups]').click();const g=await p.locator('#list [data-person]').first();await g.click();await p.waitForTimeout(500);
 chk(w+' group row opens groups.html',p.url().includes('groups.html'),p.url());
 // Escape key in chat from home
 await p.goto(BASE+'chat.html?chat=dev&from=home');await p.waitForTimeout(300);await p.locator('#convTitle').focus();await p.keyboard.press('Escape');await p.waitForTimeout(600);chk(w+' Escape in from=home chat returns home',p.url().includes('home.html'),p.url());
 chk(w+' home flow no runtime errors',!p.errs.length,p.errs.join('|'));await p.close()})}
// 4. 760px home boundary and desktop home
await step('home1440',async()=>{const p=await page(b);await p.goto(BASE+'home.html');await p.waitForTimeout(900);await p.locator('#list [data-person=arjun]').click();await p.waitForTimeout(400);
 chk('1440 home click stays on home with detail',p.url().includes('home.html'),p.url());await shot(p,'home-1440');
 const src=await p.locator('#convo').getAttribute('src');chk('home 1440 embeds chat.html with embed=1',/chat\.html\?chat=arjun&embed=1/.test(src||''),src);
 const fr=p.frameLocator('#convo');chk('home 1440 embedded chat has no back button or harness',!(await fr.locator('.back-list').isVisible())&&!(await fr.locator('.harness').isVisible()));await p.close()});
await step('chatlinks',async()=>{const p=await page(b);await p.goto(BASE+'chat.html?chat=arjun');await p.waitForTimeout(400);const h=await p.locator('#mobilePreview').getAttribute('href');
 chk('chat.html mobilePreview href resolves to mobile.html chat=arjun',/mobile\.html\?.*chat=arjun/.test(h),h);await p.locator('#mobilePreview').click();await p.waitForTimeout(600);
 chk('mobilePreview click loads mobile iframe with Arjun',(await p.frameLocator('#phone').locator('#convTitle').innerText()).includes('Arjun'));
 // mobile.html at narrow
 await p.setViewportSize({width:390,height:844});await p.goto(BASE+'mobile.html?chat=sana');await p.waitForTimeout(600);
 chk('mobile.html at 390: fills viewport no overflow',(o=>o.sw<=o.iw)(await ovf(p)),JSON.stringify(await ovf(p)));
 chk('mobile.html at 390: iframe composer visible',await p.frameLocator('#phone').locator('#ta').isVisible());
 const ib=await p.locator('#phone').boundingBox();chk('mobile.html at 390: iframe within viewport',ib.x>=0&&ib.x+ib.width<=390&&ib.y+ib.height<=844+1,JSON.stringify(ib));await shot(p,'mobile-page-390');
 await p.setViewportSize({width:320,height:640});await p.waitForTimeout(400);chk('mobile.html at 320: no overflow',(o=>o.sw<=o.iw)(await ovf(p)),JSON.stringify(await ovf(p)));await shot(p,'mobile-page-320');
 chk('links runtime errors',!p.errs.length,p.errs.join('|'));await p.close()});
// N. Phone preview of Home: the call outlives every page inside the bezel, and there is never a second strip
for(const dark of [false,true])await step('mobile-home-callbar-'+(dark?'dark':'light'),async()=>{const L=dark?'dark':'light';const p=await page(b,{dark});await p.goto(BASE+'mobile.html?screen=home');await p.waitForTimeout(900);
 const fr=()=>p.frames().find(f=>/refined\/(home|chat|groups)\.html/.test(f.url()));
 const strips=async()=>{let n=0;for(const f of p.frames()){try{n+=await f.evaluate(()=>[...document.querySelectorAll('.t-callbar')].filter(e=>e.getClientRects().length&&!e.hidden).length)}catch{}}return n};
 const secs=t=>{const m=/(\d\d):(\d\d)/.exec(t||'');return m?+m[1]*60+ +m[2]:-1};
 await fr().locator('#list [data-person=arjun]').click();await p.waitForTimeout(800);
 chk(L+' mobile home: Arjun opens chat.html inside the bezel',/chat\.html\?chat=arjun&from=home/.test(fr().url()),fr().url());
 await fr().locator('#callBtn').click();await p.waitForTimeout(3300);chk(L+' mobile home: overlay up, no strip yet',await fr().locator('#callOv').isVisible()&&await strips()===0);
 await fr().locator('#callMin').click();await p.waitForTimeout(150);const t0=secs(await fr().locator('#callStrip').textContent());chk(L+' mobile home: one strip in the conversation',await strips()===1&&t0>=0);
 await fr().getByRole('button',{name:'Back to chats',exact:true}).click();await p.waitForTimeout(900);
 const t1=await fr().locator('#callStrip').textContent();
 chk(L+' mobile home: Back → Home inside the bezel keeps the strip, timer continues',/home\.html/.test(fr().url())&&/^Arjun Oommen · \d\d:\d\d$/.test(t1)&&secs(t1)>=t0&&await strips()===1,t1+' '+t0);
 const r=await fr().evaluate(()=>[document.querySelector('#callStrip').getBoundingClientRect().toJSON(),document.querySelector('.mobile-bar').getBoundingClientRect().top]);
 chk(L+' mobile home: strip at the top of the phone, header below it',r[0].top===0&&r[0].height===40&&Math.round(r[1])===40,JSON.stringify(r));
 await p.waitForTimeout(300);await p.screenshot({path:path.join(SS,'callbar-mobile-home-'+L+'.png')});
 await fr().locator('#mobile-nav [data-tab=calls]').click();chk(L+' mobile home: strip on Calls tab',await fr().locator('#callStrip').isVisible()&&await strips()===1);
 await fr().locator('#callStrip').click();await p.waitForTimeout(900);
 chk(L+' mobile home: strip → conversation with overlay open',/chat\.html\?chat=arjun&from=home$/.test(fr().url())&&await fr().locator('#callOv').isVisible()&&await strips()===0,fr().url());
 if(!dark)await p.screenshot({path:path.join(SS,'callbar-mobile-overlay-after-strip.png')});
 await fr().locator('#callEnd').click();await p.waitForTimeout(150);await fr().getByRole('button',{name:'Back to chats',exact:true}).click();await p.waitForTimeout(900);
 chk(L+' mobile home: after End no strip anywhere',/home\.html/.test(fr().url())&&await strips()===0);
 chk(L+' mobile home: no AudioContext',(await p.evaluate(()=>0))===0&&!(await fr().evaluate(()=>window.__aud&&window.__aud.ctxs.length)));
 chk(L+' mobile home: no runtime errors',!p.errs.length,p.errs.join('|'));await p.close()});
// O. mobile.html?screen=chat (embed=1 page): its own strip is the only one
await step('mobile-chat-embed-callbar',async()=>{const p=await page(b);await p.goto(BASE+'mobile.html?chat=meera');await p.waitForTimeout(700);const f=p.frameLocator('#phone');
 await f.locator('#callBtn').click();await p.waitForTimeout(200);await f.locator('#callMin').click();
 chk('embed chat: exactly one strip, inside the frame',await f.locator('#callStrip').isVisible()&&await p.locator('.t-callbar').count()===0);
 await f.locator('#callStrip').click();await f.locator('#callEnd').click();await p.close()});
await b.close();
fs.writeFileSync(path.join(__dirname,'mobile-chat-results.json'),JSON.stringify(results,null,2));
const f=results.filter(r=>!r.ok);console.log(`${results.length-f.length}/${results.length} passed`);
fs.mkdirSync('/tmp/tinline-mobile',{recursive:true});
fs.writeFileSync('/tmp/tinline-mobile/findings.txt',f.length?f.map(r=>`FAIL: ${r.name}\n  ${r.detail}`).join('\n'):'No failures');
})();
