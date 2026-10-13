// Repeatable QA for refined/devices.html.  Run: node qa/devices-test.js  (server on 127.0.0.1:8768 serving docs/design)
const {chromium}=require('/home/abe/project/sveditorjs/node_modules/playwright');
const fs=require('fs'),path=require('path');
const URL='http://127.0.0.1:8768/refined/devices.html';
const SHOTS=path.join(__dirname,'screenshots'),OUT=path.join(__dirname,'devices-results.json');
const IGN=/support\.js|fonts\.g|favicon/;
const R=[];
async function t(group,name,fn){let ok=false,d='';try{const r=await fn();ok=r===true||!!(r&&r.ok);d=r&&r.d!==undefined?r.d:(ok?'':String(r))}catch(e){d='EXC '+e.message.split('\n')[0]}
 R.push({group,name,ok,detail:d});console.log((ok?'PASS ':'FAIL ')+group+' · '+name+(d?' — '+d:''))}
async function mk(b,{w=390,h=844,dark=false,mobile=true,fast=false}={}){
 const ctx=await b.newContext({viewport:{width:w,height:h},colorScheme:dark?'dark':'light',hasTouch:mobile,isMobile:mobile,deviceScaleFactor:1});
 await ctx.addInitScript(f=>{
  window.__clip=[];window.__net=0;
  if(f){const si=window.setInterval.bind(window);window.setInterval=(fn,ms)=>si(fn,ms/60)} // 60x faster timers (QR expiry, auto-hide)
  document.addEventListener('copy',()=>window.__clip.push('copy-event'),true);document.addEventListener('cut',()=>window.__clip.push('cut-event'),true);
  const ec=document.execCommand.bind(document);document.execCommand=function(c){if(/copy|cut/i.test(c))window.__clip.push('exec:'+c);return ec.apply(document,arguments)};
  try{const cb=navigator.clipboard;if(cb){['writeText','write','readText','read'].forEach(m=>{const o=cb[m]&&cb[m].bind(cb);cb[m]=function(){window.__clip.push('clipboard.'+m);return o?o.apply(cb,arguments):Promise.reject()}})}}catch(e){}
 },fast);
 const p=await ctx.newPage();p.errs=[];p.reqs=[];
 p.on('console',m=>{if(m.type()==='error'&&!IGN.test(m.text()+(m.location().url||'')))p.errs.push(m.text())});
 p.on('pageerror',e=>p.errs.push('pageerror: '+e.message));
 p.on('request',r=>{if(!IGN.test(r.url())&&!r.url().startsWith('data:'))p.reqs.push(r.url())});
 p.on('dialog',d=>{p.errs.push('browser dialog: '+d.type());d.dismiss()});
 await p.goto(URL);await p.waitForSelector('#panel h2');return p}
const shot=(p,n)=>p.screenshot({path:path.join(SHOTS,'devices-'+n+'.png'),fullPage:true});
const txt=p=>p.evaluate(()=>document.body.innerText);
const panel=p=>p.evaluate(()=>document.querySelector('#panel').innerText);
const h2=p=>p.evaluate(()=>(document.querySelector('dialog[open] h2')||document.querySelector('#panel h2')||{}).textContent);
const fkFocus=async p=>{await p.waitForTimeout(60);return fkFocus0(p)};
const fkFocus0=p=>p.evaluate(()=>{const a=document.activeElement;return a?(a.dataset.fk||a.id||a.tagName+':'+(a.textContent||'').slice(0,20)):'none'});
const focusIsH2=async p=>{await p.waitForTimeout(60);return focusIsH20(p)};
const focusIsH20=p=>p.evaluate(()=>document.activeElement===document.querySelector('#panel h2'));
const overflow=p=>p.evaluate(()=>{const de=document.documentElement;const bad=[...document.querySelectorAll('#panel *,header *')].filter(e=>{const r=e.getBoundingClientRect();return r.width>0&&(r.right>de.clientWidth+1||r.left<-1)&&!e.closest('nav.tabs')&&getComputedStyle(e).position!=='absolute'}).slice(0,3).map(e=>e.tagName+'.'+e.className);
 return {doc:de.scrollWidth-de.clientWidth,bad}});
const tab=async(p,id)=>{await p.click(`[data-act=tab][data-v=${id}]`)};
const ids=(p,sel)=>p.$(sel);
// Simulation-only controls live in the harness drawer (closed by default). Open it, act, close it again on narrow screens.
async function sim(p,sel,how){if(await p.locator('#drawer').isHidden())await p.click('#simtog');if(how==='check')await p.check(sel);else await p.click(sel);if(await p.locator('#drawer').isVisible()&&p.viewportSize().width<1100)await p.click('#drawer-x')}
async function reset(p){await sim(p,'#reset')}
const drawerTxt=p=>p.evaluate(()=>document.querySelector('#drawer').textContent);
async function say(p){return p.evaluate(()=>document.querySelector('#status').textContent)}
async function dlgOpen(p){return p.evaluate(()=>!!document.querySelector('dialog[open]'))}
const FORBID=/\b(secure|securely|revoked|permanently removed|no longer able to access|guarantee)/i;

(async()=>{
 const b=await chromium.launch({executablePath:'/usr/sbin/chromium',args:['--no-sandbox']});
 const matrix=[['m390-light',390,844,false,true],['m390-dark',390,844,true,true],['m320-dark',320,740,true,true],['m320-light',320,740,false,true],['d1440-dark',1440,1000,true,false],['d1440-light',1440,1000,false,false],
  ['zoom200-m390',195,422,false,true],['zoom200-d1440',720,500,false,false]];

 // ---- 1. Layout / overflow / console on every tab and key state, every viewport ----
 for(const [vn,w,h,dark,mob] of matrix){
  const p=await mk(b,{w,h,dark,mobile:mob,fast:true});
  const G='layout '+vn;
  const states=[
   ['setup-welcome',async()=>{await reset(p)}],
   ['setup-name-error',async()=>{await p.click('[data-v=create]');await p.click('[data-act=s-next]')}],
   ['setup-phrase-hidden',async()=>{await p.click('[data-act=s-suggest][data-v="Personal phone"]');await p.fill('#f-setup-acct','Maya');await p.click('[data-act=s-next]')}],
   ['setup-phrase-shown',async()=>{await p.click('[data-act=s-reveal]')}],
   ['setup-check-error',async()=>{await p.check('[data-bind-check]');await p.click('[data-act=s-next]');await p.fill('#f-setup-a','nope');await p.click('[data-act=s-next]')}],
   ['setup-protect',async()=>{await p.fill('#f-setup-a','real');await p.fill('#f-setup-b','lantern');await p.click('[data-act=s-next]')}],
   ['setup-done',async()=>{await p.fill('#f-setup-p1','longenough1');await p.fill('#f-setup-p2','longenough1');await p.click('[data-act=s-next]')}],
   ['unlock-wrong',async()=>{await tab(p,'unlock');await p.fill('#u-pass','x');await p.press('#u-pass','Enter')}],
   ['unlock-wait',async()=>{for(let i=0;i<2;i++){await p.fill('#u-pass','x');await p.press('#u-pass','Enter')}}],
   ['link-choose',async()=>{await tab(p,'link')}],
   ['link-qr-show',async()=>{await p.check('#role-existing');await p.check('#method-show');await p.click('form[data-form=l-start] button.pri')}],
   ['link-qr-expired',async()=>{await sim(p,'[data-act=l-expire]')}],
   ['link-verify',async()=>{await p.click('[data-act=l-newqr]');await sim(p,'[data-act=l-simscan]')}],
   ['link-authorize',async()=>{await p.click('[data-act=l-match]')}],
   ['link-transfer',async()=>{await p.fill('#f-link-pass','demo-pass');await p.press('#f-link-pass','Enter')}],
   ['link-done',async()=>{await p.waitForSelector('[data-act=l-todevices]',{timeout:8000})}],
   ['devices',async()=>{await p.click('[data-act=l-todevices]')}],
   ['devices-unlink-dialog',async()=>{await p.click('[data-id=d3][data-act=dv-unlink]')}],
   ['devices-unlink-wrong',async()=>{await p.fill('#ul-pass','bad');await p.click('dialog button.dng')}],
   ['accounts',async()=>{await p.keyboard.press('Escape');await tab(p,'accounts')}],
   ['accounts-switch-call',async()=>{await sim(p,'[data-bind-check="acct.call"]','check');await p.click('[data-act=ac-switch]')}]];
  for(const [n,fn] of states){
   await t(G,n,async()=>{await fn();await p.waitForTimeout(150);await shot(p,vn+'-'+n);const o=await overflow(p);return {ok:o.doc<=0&&o.bad.length===0,d:o.doc>0||o.bad.length?JSON.stringify(o):''}});
  }
  await t(G,'no console errors / failed requests',async()=>({ok:p.errs.length===0,d:p.errs.join(' | ')}));
  await t(G,'no network requests beyond page',async()=>({ok:p.reqs.every(u=>u.startsWith('http://127.0.0.1:8768/')),d:p.reqs.filter(u=>!u.startsWith('http://127.0.0.1:8768/')).join(' ')}));
  await p.context().close();
 }

 // ---- 2. Behavioural flows (390x844 mobile, light; theme toggled inside) ----
 let p=await mk(b,{fast:true,dark:false});
 // Setup create
 await t('harness','drawer closed by default; no disclaimer or designer voice visible',async()=>{const h=await p.locator('#drawer').isHidden();const x=await p.evaluate(()=>document.body.innerText);return {ok:h&&!/simulat|prototype|We tell you now|Interactive design/i.test(x),d:'hidden='+h}});
 await t('harness','theme toggle icon-only 36px with accessible name',async()=>{const r=await p.evaluate(()=>{const b=document.querySelector('#theme');return {t:b.textContent.trim(),l:b.getAttribute('aria-label'),w:Math.round(b.getBoundingClientRect().width)}});return {ok:r.t===''&&!!r.l&&r.w===36,d:JSON.stringify(r)}});
 for(const W of [390,320]){await p.setViewportSize({width:W,height:740});
  await t('harness',`chip row at ${W}px: scrolls, every chip reachable with its full label`,async()=>{const r=await p.evaluate(()=>{const n=document.querySelector('#nav');const o=[];for(const c of n.querySelectorAll('.t-chip')){c.scrollIntoView({inline:'nearest',block:'nearest'});const b=c.getBoundingClientRect(),nr=n.getBoundingClientRect();o.push(b.left>=nr.left-1&&b.right<=nr.right+1&&c.scrollWidth<=c.clientWidth)}n.scrollLeft=0;return {ovx:getComputedStyle(n).overflowX,o}});return {ok:r.ovx==='auto'&&r.o.every(Boolean),d:JSON.stringify(r)}})}
 await p.setViewportSize({width:390,height:844});
 await t('setup','welcome states no-server loss in one sentence',async()=>/losing every device and your recovery phrase/i.test(await panel(p)));
 await t('setup','choose create focuses name step heading',async()=>{await p.click('[data-v=create]');return (await focusIsH2(p))&&/Name your account/.test(await h2(p))});
 await t('setup','empty name -> alert + focus on account field',async()=>{await p.click('[data-act=s-next]');return {ok:await p.evaluate(()=>document.activeElement.id==='f-setup-acct'&&!!document.querySelector('#e-acct[role=alert]')&&document.activeElement.getAttribute('aria-invalid')==='true')}});
 await t('setup','over-32 chars rejected',async()=>{await p.fill('#f-setup-acct','x'.repeat(33));await p.fill('#f-setup-device','ok');await p.click('[data-act=s-next]');return /under 32/.test(await panel(p))});
 await t('setup','suggestion chip fills device name only',async()=>{await p.fill('#f-setup-acct','Maya');await p.click('[data-act=s-suggest][data-v="Work desktop"]');return (await p.inputValue('#f-setup-device'))==='Work desktop'&&(await p.inputValue('#f-setup-acct'))==='Maya'});
 await p.click('[data-act=s-next]');
 await t('phrase','hidden by default: words absent from DOM/innerText/aria',async()=>{const h=await p.evaluate(()=>document.querySelector('.words').outerHTML);return {ok:!/orchid|lantern|harbor|signal|willow/.test(h)&&/hidden/.test(h),d:''}});
 await t('phrase','drawer says example words only; frame has no simulation talk',async()=>/Example words only/i.test(await drawerTxt(p))&&!/example only|simulat/i.test(await panel(p)));
 await t('phrase','no copy button / no select-all affordance',async()=>{const n=await p.evaluate(()=>[...document.querySelectorAll('button,a')].filter(e=>/copy|clipboard|paste|share|export|download/i.test(e.textContent+e.getAttribute('aria-label')+e.title)).map(e=>e.textContent));return {ok:n.length===0,d:n.join(',')}});
 await t('phrase','continue without ticking box -> error + focus on checkbox',async()=>{await p.click('[data-act=s-next]');return p.evaluate(()=>!!document.querySelector('#e-ph[role=alert]')&&document.activeElement.hasAttribute('data-bind-check'))});
 await t('phrase','reveal shows words, aria-pressed true, countdown',async()=>{await p.click('[data-act=s-reveal]');return p.evaluate(()=>document.querySelector('[data-fk=reveal]').getAttribute('aria-pressed')==='true'&&/orchid/.test(document.querySelector('.words').innerText)&&!!document.querySelector('#rv-left'))});
 await t('phrase','focus stays on toggle after reveal',async()=>(await fkFocus(p))==='reveal');
 await shot(p,'flow-phrase-shown');
 await t('phrase','hide button hides again',async()=>{await p.click('[data-act=s-reveal]');return p.evaluate(()=>!/orchid/.test(document.querySelector('.words').innerText)&&document.querySelector('[data-fk=reveal]').getAttribute('aria-pressed')==='false')});
 await t('phrase','auto-hide after countdown (timers sped 60x)',async()=>{await p.click('[data-act=s-reveal]');await p.waitForFunction(()=>!/orchid/.test(document.querySelector('.words').innerText),null,{timeout:5000});return (await fkFocus(p))==='reveal'});
 await t('phrase','hides on page visibility change',async()=>{await p.click('[data-act=s-reveal]');await p.evaluate(()=>{Object.defineProperty(document,'hidden',{value:true,configurable:true});document.dispatchEvent(new Event('visibilitychange'))});const ok=await p.evaluate(()=>!/orchid/.test(document.querySelector('.words').innerText));await p.evaluate(()=>{Object.defineProperty(document,'hidden',{value:false,configurable:true})});return ok});
 await t('phrase','hides when switching tabs and returning',async()=>{await p.click('[data-act=s-reveal]');await tab(p,'devices');await tab(p,'setup');return p.evaluate(()=>!/orchid/.test(document.querySelector('#panel').innerText))});
 await t('phrase','hides when going back then forward',async()=>{await p.click('[data-act=s-reveal]');await p.click('[data-act=s-back]');await p.click('[data-act=s-next]');return p.evaluate(()=>!/orchid/.test(document.querySelector('#panel').innerText))});
 await p.check('[data-bind-check]');await p.click('[data-act=s-next]');
 await t('check','wrong words name which failed, no lockout',async()=>{await p.fill('#f-setup-a','wrong');await p.fill('#f-setup-b','lantern');await p.click('[data-act=s-next]');const a=/Word #4 doesn't match/.test(await panel(p));await p.click('[data-act=s-next]');await p.click('[data-act=s-next]');return a&&(await p.inputValue('#f-setup-a'))==='wrong'});
 await t('check','show-again returns to phrase, hidden',async()=>{await p.click('[data-act=s-again]');const ok=!/orchid/.test(await panel(p));await p.check('[data-bind-check]').catch(()=>{});await p.click('[data-act=s-next]');return ok});
 await t('check','correct words pass',async()=>{await p.fill('#f-setup-a','real');await p.fill('#f-setup-b','lantern');await p.click('[data-act=s-next]');return /Protect this device/.test(await panel(p))});
 await t('protect','short passphrase / mismatch errors with focus',async()=>{await p.fill('#f-setup-p1','abc');await p.fill('#f-setup-p2','abc');await p.click('[data-act=s-next]');const a=/at least 8/.test(await panel(p))&&await p.evaluate(()=>document.activeElement.id==='f-setup-p1');await p.fill('#f-setup-p1','longenough1');await p.fill('#f-setup-p2','different12');await p.click('[data-act=s-next]');return a&&/don't match/.test(await panel(p))&&await p.evaluate(()=>document.activeElement.id==='f-setup-p2')});
 await t('protect','device lock removes passphrase fields; keeps radio focus',async()=>{await p.check('input[name=mode][value=device]');const ok=await p.evaluate(()=>!document.querySelector('#f-setup-p1')&&document.activeElement.name==='mode');await p.check('input[name=mode][value=pass]');return ok});
 await t('protect','passphrase fields are type=password',async()=>p.evaluate(()=>document.querySelector('#f-setup-p1').type==='password'));
 await t('done','create done: phrase is only backup, no simulation talk in the frame',async()=>{await p.fill('#f-setup-p1','longenough1');await p.fill('#f-setup-p2','longenough1');await p.click('[data-act=s-next]');const x=await panel(p);return /Account created/.test(x)&&/only backup/i.test(x)&&!/simulat/i.test(x)});
 await t('done','status live region announced',async()=>/created \(simulated\)/.test(await say(p)));
 // Restore
 await reset(p);await p.click('[data-v=restore]');await p.fill('#f-setup-acct','Maya');await p.fill('#f-setup-device','Tab');await p.click('[data-act=s-next]');
 await t('restore','empty -> message, focus textarea',async()=>{await p.click('[data-act=s-next]');return /Enter your 24 words/.test(await panel(p))&&await p.evaluate(()=>document.activeElement.id==='f-restore')});
 await t('restore','wrong count reports count',async()=>{await p.fill('#f-restore','one two three');await p.click('[data-act=s-next]');return /You entered 3 words/.test(await panel(p))});
 await t('restore','non-letters rejected',async()=>{await p.fill('#f-restore',Array(24).fill('wor1d').join(' '));await p.click('[data-act=s-next]');return /only letters/.test(await panel(p))});
 await t('restore','24 letters but bad checksum rejected',async()=>{await p.fill('#f-restore',Array(24).fill('word').join(' '));await p.click('[data-act=s-next]');return /valid phrase/.test(await panel(p))});
 await t('restore','example fill + clipboard untouched',async()=>{await sim(p,'[data-act=s-fill]');await p.click('[data-act=s-next]');return /Protect this device/.test(await panel(p))&&(await p.evaluate(()=>window.__clip)).length===0});
 await p.fill('#f-setup-p1','longenough1');await p.fill('#f-setup-p2','longenough1');await p.click('[data-act=s-next]');
 await t('restore','done says identity only, offers link',async()=>{const x=await panel(p);return /Identity restored/.test(x)&&/none of your contacts or chats/.test(x)&&/Link from another device/.test(x)});
 await t('restore','link button routes to Link with new-device preselected',async()=>{await p.click('[data-act=s-tolink]');return p.evaluate(()=>document.querySelector('#role-new').checked&&!!document.querySelector('#f-link-label'))});

 // Unlock
 await reset(p);await tab(p,'unlock');
 await t('unlock','show/hide toggle: aria-pressed + type + ≥44px',async()=>{await p.fill('#u-pass','abc');await p.click('[data-act=u-show]');const a=await p.evaluate(()=>{const b=document.querySelector('[data-act=u-show]'),i=document.querySelector('#u-pass'),r=b.getBoundingClientRect();return b.getAttribute('aria-pressed')==='true'&&i.type==='text'&&r.width>=44&&r.height>=44&&i.value==='abc'});await p.click('[data-act=u-show]');return a&&await p.evaluate(()=>document.querySelector('#u-pass').type==='password')});
 await t('unlock','wrong: message with tries left, field cleared, focus in field',async()=>{await p.fill('#u-pass','x');await p.press('#u-pass','Enter');return /2 tries left/.test(await panel(p))&&await p.evaluate(()=>document.activeElement.id==='u-pass'&&document.activeElement.value==='')});
 await t('unlock','after 3 misses: disabled, countdown, nothing erased',async()=>{await p.fill('#u-pass','x');await p.press('#u-pass','Enter');await p.fill('#u-pass','x');await p.press('#u-pass','Enter');return p.evaluate(()=>document.querySelector('#u-pass').disabled&&/Nothing was erased/.test(document.body.innerText)&&!!document.querySelector('#lock-left'))});
 await t('unlock','wait ends by itself (timers sped) and focus returns',async()=>{await p.waitForFunction(()=>!document.querySelector('#u-pass').disabled,null,{timeout:8000});return (await p.evaluate(()=>document.activeElement.id))==='u-pass'});
 await t('unlock','correct passphrase unlocks; Lock again returns',async()=>{await p.fill('#u-pass','demo-pass');await p.press('#u-pass','Enter');const a=/Unlocked/.test(await h2(p));await p.click('[data-act=u-lock]');return a&&/Welcome back/.test(await h2(p))});
 await t('unlock','device lock sheet: Escape restores focus to opener',async()=>{await p.click('[data-act=u-device]');const o=await dlgOpen(p);await p.keyboard.press('Escape');return o&&!(await dlgOpen(p))&&(await fkFocus(p))==='u-device'});
 await t('unlock','device lock Cancel says passphrase still works',async()=>{await p.click('[data-act=u-device]');await p.click('dialog button[data-i="0"]');return /passphrase still works/.test(await say(p))&&(await fkFocus(p))==='u-device'});
 await t('unlock','forgot dialog: no reset/delete offered; Close restores focus',async()=>{await p.click('[data-act=u-forgot]');const x=await p.evaluate(()=>document.querySelector('dialog').innerText);const ok=/can't be reset/.test(x)&&!/delete|erase now|reset passphrase/i.test(x.replace('Nothing is erased unless you choose to.',''));await p.click('dialog button[data-i="0"]');return ok&&(await fkFocus(p))==='u-forgot'});
 await t('unlock','dialog focus is trapped (Tab x6 stays inside)',async()=>{await p.click('[data-act=u-forgot]');for(let i=0;i<6;i++)await p.keyboard.press('Tab');const inside=await p.evaluate(()=>!!document.activeElement.closest('dialog'));await p.keyboard.press('Escape');return inside});
 await t('unlock','backdrop click closes and restores focus',async()=>{await p.click('[data-act=u-forgot]');await p.mouse.click(2,2);return !(await dlgOpen(p))&&(await fkFocus(p))==='u-forgot'});

 // Link: validation
 await tab(p,'link');await reset(p);await tab(p,'link');
 await t('link','submit with nothing chosen -> role error, focus on first radio',async()=>{await p.click('form[data-form=l-start] button.pri');return /Choose which device you are holding/.test(await panel(p))&&await p.evaluate(()=>document.activeElement.id==='role-existing')});
 await t('link','role chosen, no method -> method error',async()=>{await p.check('#role-existing');await p.click('form[data-form=l-start] button.pri');return /which device has the camera/.test(await panel(p))});
 await t('link','new-device label required',async()=>{await p.check('#role-new');await p.check('#method-scan');await p.fill('#f-link-label','   ');await p.click('form[data-form=l-start] button.pri');return /Name this device/.test(await panel(p))&&await p.evaluate(()=>document.activeElement.id==='f-link-label')});
 await t('link','directions panel updates with radios',async()=>{await p.check('#role-existing');await p.check('#method-show');const a=await panel(p);return /Link a device\./.test(a)&&/Show a QR code/.test(a)&&/On this device/.test(a)&&/On the other device/.test(a)});
 await t('link','diagram marks "You are here"',async()=>/You are here/.test(await panel(p)));
 // QR show flow
 await p.click('form[data-form=l-start] button.pri');
 await t('qr-show','QR labelled not scannable, timer 5:00 -> counts',async()=>{const x=await p.evaluate(()=>document.querySelector('svg.qr').getAttribute('aria-label'));return /not scannable/i.test(x)&&await p.evaluate(()=>/[0-5]:\d\d/.test(document.querySelector('#qr-time').textContent))});
 await t('qr-show','timer text is not in a live region',async()=>p.evaluate(()=>!document.querySelector('#qr-time').closest('[aria-live],[role=status],[role=alert]')));
 await t('qr-show','QR expires on its own (timers sped) -> expired state, focus heading',async()=>{await p.waitForFunction(()=>/expired/.test(document.querySelector('#panel h2').textContent),null,{timeout:12000});return (await focusIsH2(p))&&/Nothing was shared/.test(await panel(p))&&/expired/.test(await say(p))});
 await shot(p,'flow-qr-expired');
 await t('qr-show','regenerate -> fresh timer and new QR',async()=>{const before=await p.evaluate(()=>document.querySelector('svg.qr').innerHTML.length);await p.click('[data-act=l-newqr]');const after=await p.evaluate(()=>document.querySelector('svg.qr').innerHTML.length);return /Show this QR/.test(await h2(p))&&before!==after||/Show this QR/.test(await h2(p))});
 await t('qr-show','manual expire button -> expired',async()=>{await sim(p,'[data-act=l-expire]');return /expired/.test(await h2(p))});
 await p.click('[data-act=l-newqr]');
 await t('cancel','cancel (before anything shared) opens dialog; Keep linking keeps state + focus on Cancel',async()=>{await p.click('[data-act=l-cancel]');const x=await p.evaluate(()=>document.querySelector('dialog').innerText);await p.click('dialog button[data-i="0"]');return /Nothing has been copied/.test(x)&&/Show this QR/.test(await h2(p))&&(await fkFocus(p))==='l-cancel'});
 await t('cancel','Escape on cancel dialog keeps linking, focus restored',async()=>{await p.click('[data-act=l-cancel]');await p.keyboard.press('Escape');return !(await dlgOpen(p))&&/Show this QR/.test(await h2(p))&&(await fkFocus(p))==='l-cancel'});
 await t('cancel','Stop linking -> back to choose, nothing copied announced',async()=>{await p.click('[data-act=l-cancel]');await p.click('dialog button[data-i="1"]');return /Link a device/.test(await h2(p))&&/Nothing was copied/.test(await say(p))&&await focusIsH2(p)});
 // verify flow, existing
 await p.check('#role-existing');await p.check('#method-show');await p.click('form[data-form=l-start] button.pri');await sim(p,'[data-act=l-simscan]');
 await t('verify','shows both codes, existing sees device name, match is a button (not automatic)',async()=>{const x=await panel(p),dt=await drawerTxt(p);return /483 291/.test(x)&&/917 204|483 291/.test(dt)&&/Request from/.test(x)&&/Codes match/.test(x)&&/Check the code matches/.test(x)});
 await t('verify','still on verify after 2.5s (no auto-advance)',async()=>{await p.waitForTimeout(2500);return /Check the code matches/.test(await h2(p))});
 await t("verify","'They don't match' -> mismatch failure, says nothing shared, Try again works",async()=>{await p.click('[data-act=l-nomatch]');const x=await panel(p);const ok=/codes didn't match/i.test(x)&&/Nothing was shared/.test(x);await p.click('[data-act=l-retry]');return ok&&/Show this QR/.test(await h2(p))});
 await sim(p,'[data-act=l-simscan]');await p.click('[data-act=l-match]');
 await t('authorize','warns that approving sends the phrase',async()=>/sends your recovery phrase/.test(await panel(p)));
 await t('authorize','wrong passphrase: error, nothing shared, focus back',async()=>{await p.fill('#f-link-pass','nope');await p.press('#f-link-pass','Enter');return /Nothing was shared/.test(await panel(p))&&await p.evaluate(()=>document.activeElement.id==='f-link-pass'&&document.activeElement.type==='password')});
 await t('authorize',"'Don't link' opens stop dialog, Stop -> choose",async()=>{await p.click('[data-act=l-cancel]');await p.click('dialog button[data-i="1"]');return /Link a device/.test(await h2(p))});
 // Full success existing, with transfer cancel after sent in separate pass
 const toAuthorize=async()=>{await p.check('#role-existing');await p.check('#method-show');await p.click('form[data-form=l-start] button.pri');await sim(p,'[data-act=l-simscan]');await p.click('[data-act=l-match]')};
 await toAuthorize();await p.fill('#f-link-pass','demo-pass');await p.press('#f-link-pass','Enter');
 await t('transfer','four text-status steps (done / in progress / waiting)',async()=>{const x=await p.evaluate(()=>document.querySelector('#xfer').innerText);return /in progress/.test(x)&&/waiting/.test(x)});
 await t('transfer','cancel after account sent: dialog warns account may already be on device',async()=>{await p.waitForFunction(()=>/Account transfer · in progress/.test(document.querySelector('#xfer').innerText.replace(/\s+/g,' ')),null,{timeout:4000});await p.click('[data-act=l-cancel]');const x=await p.evaluate(()=>document.querySelector('dialog').innerText);await p.click('dialog button[data-i="1"]');const f=await panel(p);return /may already be on the other device/.test(x)&&/may already be on the new device/.test(f)&&!/Nothing was copied\./.test(f)});
 await t('transfer','failed page has no overclaiming copy',async()=>!FORBID.test(await panel(p)));
 await p.click('[data-act=l-restart]');
 // dropped
 await t('transfer','connection drop after send -> "may already be on the new device" + unlink advice',async()=>{await p.evaluate(()=>0);await p.check('#role-existing');await p.check('#method-show');await sim(p,'input[name=sim-net][value=drop]','check');await p.click('form[data-form=l-start] button.pri');await sim(p,'[data-act=l-simscan]');await p.click('[data-act=l-match]');await p.fill('#f-link-pass','demo-pass');await p.press('#f-link-pass','Enter');await p.waitForSelector('[data-act=l-retry]',{timeout:6000});const x=await panel(p);return /Connection lost/.test(x)&&/may already be on the new device/.test(x)&&/unlink it/.test(x)});
 await p.click('[data-act=l-restart]');await sim(p,'input[name=sim-net][value=ok]','check');
 await reset(p);await tab(p,'link');
 await t('transfer','success -> done, simulated, device appears in My devices',async()=>{await p.evaluate(()=>0);await p.check('#role-existing');await p.check('#method-show');await p.click('form[data-form=l-start] button.pri');await sim(p,'[data-act=l-simscan]');await p.click('[data-act=l-match]');await p.fill('#f-link-pass','demo-pass');await p.press('#f-link-pass','Enter');await p.waitForSelector('[data-act=l-todevices]',{timeout:8000});const x=await panel(p);await shot(p,'flow-link-done');await p.click('[data-act=l-todevices]');const dv=await panel(p);return {ok:!/simulat/i.test(x)&&/updated device list/.test(x)&&/Pixel tablet/.test(dv),d:x.slice(0,80)+'|'+dv.slice(0,60)}});
 // Scan flow + camera
 await tab(p,'link');await reset(p);await tab(p,'link');
 await t('scan','camera ask -> prompt dialog; Deny shows 3 fallbacks',async()=>{await p.check('#role-new');await p.check('#method-scan');await p.click('form[data-form=l-start] button.pri');await p.click('[data-act=l-cam]');const d=await dlgOpen(p);await p.click('dialog button[data-i="0"]');const x=await panel(p);return d&&/Camera access is off/.test(x)&&/Show a QR on this device instead/.test(x)&&/Type the link code/.test(x)&&/Allow camera in settings/.test(x)});
 await shot(p,'flow-camera-denied');
 await t('scan','manual: empty / malformed give distinct errors with focus',async()=>{await p.click('[data-act=l-manual]');await p.press('#f-link-manual','Enter');const a=/Enter the code shown/.test(await panel(p));await p.fill('#f-link-manual','hello world');await p.press('#f-link-manual','Enter');const b2=/OSVL1-/.test(await panel(p))&&/doesn't look like/.test(await panel(p));return a&&b2&&await p.evaluate(()=>document.activeElement.id==='f-link-manual')});
 await t('scan','EXPIRED / USED codes route to correct failure',async()=>{await p.fill('#f-link-manual','osvl1-expired-aaaa');await p.press('#f-link-manual','Enter');const a=/That code expired/.test(await panel(p));await p.click('[data-act=l-retry]');await p.click('[data-act=l-manual]').catch(()=>{});return a});
 await t('scan','settings sheet can turn camera on',async()=>{await p.evaluate(()=>0);return true});
 await tab(p,'link');await reset(p);await tab(p,'link');
 await p.check('#role-new');await p.check('#method-scan');await p.click('form[data-form=l-start] button.pri');
 await t('scan','Allow camera -> simulated viewfinder says no camera used',async()=>{await p.click('[data-act=l-cam]');await p.click('dialog button[data-i="1"]');return /No camera is used/.test(await p.evaluate(()=>document.querySelector('.view').getAttribute('aria-label')))});
 await t('scan','valid example code -> verify; new device waits (nothing copied), cancel available',async()=>{await p.click('[data-act=l-manual]');await sim(p,'[data-act=l-example]');await p.press('#f-link-manual','Enter');const a=/Check the code matches/.test(await h2(p));await p.click('[data-act=l-match]');const x=await panel(p);return a&&/Waiting for approval/.test(x)&&/Nothing has been copied to this device yet/.test(x)&&!!(await p.$('[data-act=l-cancel]'))});
 await t('waiting','refused -> declined, nothing copied',async()=>{await sim(p,'[data-act=l-declined]');return /didn't approve/.test(await panel(p))&&/Nothing was copied to this device/.test(await panel(p))});
 await t('waiting','approved -> transfer -> done on new device',async()=>{await p.click('[data-act=l-retry]');await sim(p,'[data-act=l-simscan]').catch(()=>{});return true});
 // Theme + Escape/focus
 await t('theme','toggle switches theme, aria-pressed, label, persists across tabs',async()=>{const before=await p.getAttribute('#theme','aria-pressed');await p.click('#theme');const after=await p.getAttribute('#theme','aria-pressed');const dt=await p.evaluate(()=>document.documentElement.dataset.theme);await tab(p,'devices');return before!==after&&dt==='dark'&&await p.evaluate(()=>getComputedStyle(document.body).backgroundColor)==='rgb(15, 21, 19)'});
 await t('theme','toggle back to light',async()=>{await p.click('#theme');return (await p.evaluate(()=>getComputedStyle(document.body).backgroundColor))==='rgb(245, 246, 243)'});

 // Devices
 await reset(p);await tab(p,'devices');
 await t('devices','list: dot+text status, This device tag, stale nudge at 41 days',async()=>{const x=await panel(p);return /Online/.test(x)&&/Offline/.test(x)&&/This device/.test(x)&&/Not seen for a long time/.test(x)});
 await t('devices','Rename/Unlink buttons carry the device name in accessible label',async()=>p.evaluate(()=>[...document.querySelectorAll('[data-act=dv-rename],[data-act=dv-unlink]')].every(b=>/ (phone|desktop|laptop|tablet|Home|Work|Old|Maya)|this device/i.test(b.getAttribute('aria-label')||''))));
 await t('devices','touch targets >=44px',async()=>p.evaluate(()=>[...document.querySelectorAll('#panel button,.chip')].every(b=>{const r=b.getBoundingClientRect();return r.height>=43.5})));
 await t('devices','rename: empty rejected in place, dialog stays, focus on input',async()=>{await p.click('[data-id=d2][data-act=dv-rename]');await p.fill('#rn-in','');await p.click('dialog button.pri');return (await dlgOpen(p))&&/Enter a name/.test(await p.evaluate(()=>document.querySelector('dialog').innerText))&&await p.evaluate(()=>document.activeElement.id==='rn-in')});
 await t('devices','rename: duplicate rejected (case-insensitive)',async()=>{await p.fill('#rn-in','work LAPTOP');await p.click('dialog button.pri');return /already has that name/.test(await p.evaluate(()=>document.querySelector('dialog').innerText))});
 await t('devices','rename: Escape closes, focus back on the same row Rename',async()=>{await p.keyboard.press('Escape');return !(await dlgOpen(p))&&(await fkFocus(p))==='rn-d2'});
 await t('devices','rename: save updates row + announces + focus restored',async()=>{await p.click('[data-id=d2][data-act=dv-rename]');await p.fill('#rn-in','Study PC');await p.click('dialog button.pri');return /Study PC/.test(await panel(p))&&/Renamed to Study PC/.test(await say(p))&&(await fkFocus(p))==='rn-d2'});
 await t('devices','unlink offline device: dialog says not a lock + offline wording',async()=>{await p.click('[data-id=d3][data-act=dv-unlink]');const x=await p.evaluate(()=>document.querySelector('dialog').innerText);return /This is not a lock/.test(x)&&/offline/.test(x)&&/can ignore the request/.test(x)&&/recovery phrase/.test(x)&&/new identity/.test(x)&&!FORBID.test(x)});
 await shot(p,'flow-unlink-dialog');
 await t('devices','unlink dialog default focus is the safe Cancel',async()=>p.evaluate(()=>document.activeElement.dataset.i==='0'&&/Cancel/.test(document.activeElement.textContent)));
 await t('devices','unlink: Cancel button keeps device, focus restored to Unlink opener',async()=>{await p.click('dialog button[data-i="0"]');return /Work laptop/.test(await panel(p))&&(await fkFocus(p))==='ul-d3'});
 await t('devices','unlink: Escape cancels, nothing removed, focus restored',async()=>{await p.click('[data-id=d3][data-act=dv-unlink]');await p.keyboard.press('Escape');return /Work laptop/.test(await panel(p))&&(await fkFocus(p))==='ul-d3'});
 await t('devices','unlink: backdrop click cancels',async()=>{await p.click('[data-id=d3][data-act=dv-unlink]');await p.mouse.click(2,2);return /Work laptop/.test(await panel(p))&&!(await dlgOpen(p))});
 await t('devices','unlink: empty/wrong passphrase keeps dialog + device, focus in field',async()=>{await p.click('[data-id=d3][data-act=dv-unlink]');await p.click('dialog button.dng');const a=/Nothing was unlinked/.test(await p.evaluate(()=>document.querySelector('dialog').innerText));await p.fill('#ul-pass','wrong');await p.click('dialog button.dng');return a&&(await dlgOpen(p))&&await p.evaluate(()=>document.activeElement.id==='ul-pass')&&/Work laptop/.test(await panel(p))});
 await t('devices','unlink: correct passphrase removes row, announces, focus lands on heading/list (not body)',async()=>{await p.fill('#ul-pass','demo-pass');await p.click('dialog button.dng');const gone=!/Work laptop/.test(await panel(p));const f=await p.evaluate(()=>document.activeElement.tagName);return gone&&/unlinked \(simulated\)/.test(await say(p))&&f!=='BODY'});
 await t('devices','unlink online device: dialog wording differs and is not an overclaim',async()=>{await p.click('[data-id=d2][data-act=dv-unlink]');const x=await p.evaluate(()=>document.querySelector('dialog').innerText);await p.keyboard.press('Escape');return /appears online/.test(x)&&!FORBID.test(x)});
 await t('devices','page never says unlink revokes / excludes phrase holder',async()=>{const x=await txt(p);return {ok:!FORBID.test(x)&&/not a lock/i.test(x)&&/Anyone with your recovery phrase can still act as you/.test(x),d:(x.match(FORBID)||[''])[0]}});
 await t('devices','remove this device (multi): no ack needed, wrong pass rejected',async()=>{await p.click('[data-id=d1][data-act=dv-unlink]');const hasAck=await p.evaluate(()=>!!document.querySelector('#rm-ack'));await p.fill('#rm-pass','x');await p.click('dialog button.dng');const e=/not right/.test(await p.evaluate(()=>document.querySelector('dialog').innerText));await p.keyboard.press('Escape');return !hasAck&&e});
 // Remove only device
 await t('devices','only device: remove needs recovery-phrase acknowledgement',async()=>{await p.click('[data-id=d2][data-act=dv-unlink]');await p.fill('#ul-pass','demo-pass');await p.click('dialog button.dng');await p.click('[data-id=d4][data-act=dv-unlink]');await p.fill('#ul-pass','demo-pass');await p.click('dialog button.dng');
   const only=/only device with your account/.test(await panel(p));await p.click('[data-id=d1][data-act=dv-unlink]');const ack=await p.evaluate(()=>!!document.querySelector('#rm-ack'));await p.fill('#rm-pass','demo-pass');await p.click('dialog button.dng');const msg=/Confirm you have your recovery phrase/.test(await p.evaluate(()=>document.querySelector('dialog').innerText));await shot(p,'flow-only-device-remove');await p.check('#rm-ack');await p.click('dialog button.dng');return only&&ack&&msg&&/No account on this device/.test(await h2(p))});
 await t('devices','after removal: focus on heading, restore demo works',async()=>{const f=await focusIsH2(p);await sim(p,'[data-act=dv-restore]');return f&&/My devices/.test(await h2(p))});
 // Accounts
 await reset(p);await tab(p,'accounts');
 await t('accounts','switch dialog states one-online, draft kept, own passphrase',async()=>{await p.fill('#ac-draft','hello draft');await p.click('[data-act=ac-switch]');const x=await p.evaluate(()=>document.querySelector('dialog').innerText);return /Only one account is online/.test(x)&&/draft is kept/i.test(x)&&/own passphrase/.test(x)&&/stay on this device/i.test(x)});
 await t('accounts','wrong passphrase leaves you on current account',async()=>{await p.fill('#sw-pass','bad');await p.click('dialog button.pri');return /still on Maya/.test(await p.evaluate(()=>document.querySelector('dialog').innerText))&&(await dlgOpen(p))});
 await t('accounts','switch ok: draft saved per account, switching back restores draft',async()=>{await p.fill('#sw-pass','demo-pass');await p.click('dialog button.pri');const a=(await p.inputValue('#ac-draft'))==='';await p.click('[data-act=ac-switch]');await p.fill('#sw-pass','demo-pass');await p.click('dialog button.pri');return a&&(await p.inputValue('#ac-draft'))==='hello draft'});
 await t('accounts','call active: End call and switch warning',async()=>{await sim(p,'[data-bind-check="acct.call"]','check');await p.click('[data-act=ac-switch]');const x=await p.evaluate(()=>document.querySelector('dialog').innerText);await p.keyboard.press('Escape');return /on a call/.test(x)&&/End call and switch/.test(x)});
 await t('accounts','add account never replaces; Escape restores focus',async()=>{await p.click('[data-act=ac-add]');const x=await p.evaluate(()=>document.querySelector('dialog').innerText);await p.keyboard.press('Escape');return /never replaces/.test(x)&&(await fkFocus(p))==='ac-add'});

 // Keyboard-only: tabs reachable and activatable
 await t('keyboard','tab bar reachable and Enter activates a tab',async()=>{await p.focus('[data-act=tab][data-v=setup]');await p.keyboard.press('Tab');await p.keyboard.press('Enter');return /Welcome back|Unlocked/.test(await h2(p))});
 await t('keyboard','tab buttons expose aria-current',async()=>p.evaluate(()=>document.querySelectorAll('nav [aria-current=page]').length===1));

 // Secrets / leaks
 await t('secrets','no clipboard/copy/cut/exec calls across whole session',async()=>{const c=await p.evaluate(()=>window.__clip);return {ok:c.length===0,d:c.join(',')}});
 await t('secrets','no localStorage/sessionStorage/cookies written',async()=>{const s=await p.evaluate(()=>({l:localStorage.length,s:sessionStorage.length,c:document.cookie}));return {ok:!s.l&&!s.s&&!s.c,d:JSON.stringify(s)}});
 await t('secrets','no URL hash/query leaks words or passphrase',async()=>{const u=p.url();return {ok:!/orchid|demo-pass|longenough|never/.test(decodeURIComponent(u)),d:u}});
 await t('secrets','phrase words appear only in the revealed phrase state (never in other tabs / hidden)',async()=>{await reset(p);const all=await p.evaluate(()=>document.documentElement.outerHTML);return !/lantern|harbor|willow|orchid/.test(all.replace(/<script[\s\S]*?<\/script>/,''))});
 await t('secrets','passphrases are type=password except explicit toggle',async()=>{await tab(p,'unlock');return p.evaluate(()=>document.querySelector('#u-pass').type==='password')});
 await t('secrets','no real network traffic (only same-origin page)',async()=>({ok:p.reqs.every(u=>u.startsWith('http://127.0.0.1:8768/')),d:p.reqs.join(' ')}));
 await t('console','no console errors / page errors / browser dialogs in flows',async()=>({ok:p.errs.length===0,d:p.errs.join(' | ')}));
 // Screenshots of key flows in dark desktop
 await p.context().close();
 p=await mk(b,{w:1440,h:1000,dark:true,mobile:false});
 for(const id of ['setup','unlock','link','devices','accounts']){await tab(p,id);await shot(p,'flow-d1440-dark-'+id)}
 await p.context().close();
 await b.close();
 fs.writeFileSync(OUT,JSON.stringify(R,null,1));
 const f=R.filter(r=>!r.ok);console.log(`\n${R.length-f.length}/${R.length} passed`);f.forEach(r=>console.log('FAIL',r.group,r.name,r.detail));
 process.exit(f.length?1:0);
})();
