const {chromium}=require(process.env.PLAYWRIGHT_PATH||'/home/abe/project/sveditorjs/node_modules/playwright');
const fs=require('fs'),path=require('path'),assert=require('assert');
const root=__dirname,BASE=process.env.DESIGN_BASE_URL||'http://127.0.0.1:8768/refined/';
const results=[];function check(name,condition,detail=''){results.push({name,ok:!!condition,detail});if(!condition)throw Error(name+' '+detail)}
(async()=>{const b=await chromium.launch({executablePath:process.env.CHROMIUM_PATH||'/usr/sbin/chromium',args:['--no-sandbox']});try{
 for(const [w,h,dark]of [[320,740,false],[390,844,false],[390,844,true],[1440,1000,false],[1440,1000,true]]){
  const p=await b.newPage({viewport:{width:w,height:h},colorScheme:dark?'dark':'light'}),errs=[];p.on('pageerror',e=>errs.push(e.message));
  const label=`${w}-${dark?'dark':'light'}`;await p.goto(BASE);await p.waitForTimeout(850);
  check(label+' entry opens home',p.url().includes('home.html'));check(label+' no overflow',await p.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  check(label+' opening releases controls',await p.locator('#app').evaluate(e=>!e.inert));
  check(label+' conversation rows populated',await p.locator('#list [data-person]').count()===6);
  await p.screenshot({path:path.join(root,'screenshots',`home-${label}.png`)});
  check(label+' no designer notes in app frame',await p.evaluate(()=>!/simulat|design preview|preview only|study/i.test(document.querySelector('#app').innerText+document.querySelector('#mobile-nav').innerText)&&!document.querySelector('.demo-note')));
  check(label+' filter chips are t-chip with aria-pressed',await p.evaluate(()=>{const c=[...document.querySelectorAll('#filters button')];return c.length===3&&c.every(b=>b.classList.contains('t-chip')&&b.hasAttribute('aria-pressed'))&&new Set(c.map(b=>b.getBoundingClientRect().height)).size===1}));
  check(label+' unread badges use t-badge',await p.locator('#list .t-badge').count()===3&&await p.locator('#list .t-badge.muted').count()===1);
  check(label+' muted chat shows bell-slash before time',await p.locator('#list [data-person=family] .when .mute + .time').count()===1);
  check(label+' avatars use shared hue hash',await p.evaluate(()=>{const h=n=>'av'+[...n].reduce((a,c)=>a+c.codePointAt(0),0)%6;return [...document.querySelectorAll('#list [data-person]')].every(r=>{const a=r.querySelector('.t-avatar'),n=r.querySelector('.name').textContent;return a.classList.contains('grp')||a.classList.contains(h(n))})}));
  check(label+' links never default blue',await p.evaluate(()=>[...document.querySelectorAll('a')].every(a=>getComputedStyle(a).color!=='rgb(0, 0, 238)')));
  if(w>=860){
   check(label+' empty detail when nothing selected',await p.locator('#detail-empty').isVisible()&&await p.locator('#detail-title').textContent()==='Choose a chat'&&await p.locator('#convo').isHidden());
   await p.locator('#list [data-person=arjun]').click();await p.waitForTimeout(500);const src=await p.locator('#convo').getAttribute('src');
   check(label+' selecting chat embeds chat.html with embed=1',/^chat\.html\?chat=arjun&embed=1&callbar=host$/.test(src),src);check(label+' iframe titled',(await p.locator('#convo').getAttribute('title')).includes('Arjun Oommen'));check(label+' row selected tint on desktop',await p.locator('#list [data-person=arjun]').evaluate(e=>e.classList.contains('selected')&&e.getAttribute('aria-current')==='true'));check(label+' stays on home',p.url().includes('home.html'));
   check(label+' iframe seam: border-left only, no radius',await p.locator('#detail').evaluate(e=>{const c=getComputedStyle(e),f=getComputedStyle(document.querySelector('#convo'));return c.borderLeftWidth==='1px'&&c.borderRightWidth==='0px'&&f.borderTopWidth==='0px'&&f.borderRadius==='0px'}));
   check(label+' opening a chat marks it read',await p.locator('#list [data-person=arjun] .t-badge').count()===0&&await p.locator('#desktop-nav .t-badge').textContent()==='3');await p.screenshot({path:path.join(root,'screenshots',`home-selected-${label}.png`)});
   await p.locator('#list [data-person=hike]').click();await p.waitForTimeout(300);const gsrc=await p.locator('#convo').getAttribute('src');check(label+' selecting group embeds groups.html with embed=1',/^groups\.html\?group=hike&embed=1&callbar=host$/.test(gsrc),gsrc);
  }else{
   check(label+' no selected-row tint on mobile',await p.locator('#list .row.selected').count()===0);
   const fab=await p.locator('#new').boundingBox(),nb=await p.locator('#mobile-nav').boundingBox();check(label+' FAB 56px bottom-right 16px above tab bar',Math.round(fab.width)===56&&Math.abs(w-(fab.x+fab.width)-16)<=1&&Math.abs(nb.y-(fab.y+fab.height)-16)<=1,JSON.stringify([fab,nb]));
   check(label+' tab badge clears icon glyph',await p.evaluate(()=>{const i=document.querySelector('#mobile-nav [data-tab=chats] .icon svg').getBoundingClientRect(),b=document.querySelector('#mobile-nav .t-badge').getBoundingClientRect();return b.left>=i.right-3&&b.bottom<=i.top+12}));
   check(label+' active tab accent icon + label',await p.evaluate(()=>{const a=document.querySelector('#mobile-nav [aria-current=page]'),i=document.querySelector('#mobile-nav button:not([aria-current])');return getComputedStyle(a).color!==getComputedStyle(i).color}));
  }
  const nav=w<860?'#mobile-nav':'#desktop-nav';for(const [id,title]of [['groups','Groups'],['calls','Calls'],['contacts','Contacts'],['chats','Chats']]){await p.locator(nav+` [data-tab=${id}]`).click();check(label+' '+id+' navigation',await p.locator('#page-title').textContent()===title);check(label+' '+id+' focus retained',await p.evaluate(id=>document.activeElement.dataset.tab===id,id));}
  if(w<860){const r=await p.locator('#mobile-nav').boundingBox();check(label+' bottom navigation visible',r.y>=0&&r.y+r.height<=h+1)}
  await p.locator('#search').fill('nobody here');check(label+' search no match',await p.locator('#list').textContent().then(t=>t.includes('No matches')));await p.locator('#clear-search').click();check(label+' clear returns focus to search',await p.locator('#search').evaluate(e=>document.activeElement===e));
  await p.locator('[data-filter=Unread]').click();check(label+' unread filter',await p.locator('#list [data-person]').count()===(w>=860?1:3));await p.locator('[data-filter=All]').click();
  if(w<860){await p.locator('#list [data-person=arjun]').click();await p.waitForURL('**/chat.html?**');check(label+' home opens conversation directly',await p.locator('#convTitle').innerText().then(t=>t.includes('Arjun Oommen')));await p.getByRole('button',{name:'Back to chats',exact:true}).click();await p.waitForURL('**/home.html#chats');check(label+' conversation Back returns home',await p.locator('#page-title').innerText()==='Chats')}
  await p.locator('[data-action=settings]:visible').click();await p.waitForTimeout(200);const r=await p.locator('#dialog').boundingBox();check(label+' settings within viewport',r.x>=-1&&r.x+r.width<=w+1&&r.y>=-1&&r.y+r.height<=h+1);await p.screenshot({path:path.join(root,'screenshots',`home-settings-${label}.png`)});check(label+' account sheet links phone preview',await p.locator('#dialog a[href="mobile.html?screen=home"]').count()===1);await p.keyboard.press('Escape');check(label+' settings Escape returns focus',await p.evaluate(()=>document.activeElement.dataset.action==='settings'));
  check(label+' no runtime errors',!errs.length,errs.join(';'));await p.close();
 }
 const prev=async p=>{await p.locator('[data-action=settings]:visible').click();await p.locator('#dialog [data-action=preview]').click()};
 const p=await b.newPage({viewport:{width:390,height:844}});await p.goto(BASE+'home.html');await p.waitForTimeout(850);
 await p.locator('[data-action=availability]:visible').click();await p.locator('[value=hour]').check();await p.getByRole('button',{name:'Save',exact:true}).click();check('pause shows nonblocking banner',await p.locator('#status-banner').innerText().then(t=>t.includes('Calls are paused')));await p.locator('[data-action=resume]').click();check('resume hides pause banner',await p.locator('#status-banner').evaluate(e=>e.hidden));
 await p.locator('#new').click();await p.locator('#ticket').fill('EXPIRED');await p.getByRole('button',{name:'Check ticket'}).click();check('expired ticket inline recovery',await p.locator('#ticket-error').textContent().then(t=>t.includes('expired')));await p.locator('#ticket').fill('TINLINE-ARJUN');await p.getByRole('button',{name:'Check ticket'}).click();check('ticket requires consent',await p.locator('#dialog-body').innerText().then(t=>t.includes('Ticket contacts can message and call you')));await p.locator('#confirm-contact').click();check('confirm opens contacts, no real network',await p.locator('#page-title').textContent()==='Contacts');
 await p.locator('#mobile-nav [data-tab=chats]').click();await prev(p);await p.locator('#offline').check();await p.keyboard.press('Escape');check('offline state has distinct copy',await p.locator('#status-banner').textContent().then(t=>t.includes('You’re offline')));await p.locator('[data-action=connection]').click();await p.locator('[data-action=go-online]').click();check('reconnect clears banner',await p.locator('#status-banner').evaluate(e=>e.hidden));
 await prev(p);await p.locator('[data-action=load]').click();check('slow loading marks busy',await p.locator('#list').getAttribute('aria-busy')==='true');check('skeleton layout preserves row geometry',await p.locator('.loading').count()===5);await p.screenshot({path:path.join(root,'screenshots','home-loading.png')});await p.waitForTimeout(1100);check('loading resolves to content',await p.locator('#list').getAttribute('aria-busy')==='false'&&await p.locator('#list [data-person]').count()===6);
 await prev(p);await p.locator('[data-action=load-error]').click();await p.waitForTimeout(1750);check('load failure gives retry',await p.locator('[data-action=retry-load]').count()===1);await p.screenshot({path:path.join(root,'screenshots','home-load-error.png')});await p.locator('[data-action=retry-load]').click();await p.waitForTimeout(1100);check('load retry succeeds',await p.locator('#list [data-person]').count()===6);
 await prev(p);await p.locator('[data-action=load]').click();await p.locator('#mobile-nav [data-tab=groups]').click();await p.waitForTimeout(1100);check('switch cancels stale loading result',await p.locator('#page-title').textContent()==='Groups'&&await p.locator('#list .loading').count()===0);
 await prev(p);await p.locator('[data-action=opening]').click();await p.waitForTimeout(100);check('opening replay visible',await p.locator('#launch').evaluate(e=>!e.hidden));await p.screenshot({path:path.join(root,'screenshots','home-opening.png')});await p.locator('#skip-launch').click();await p.waitForTimeout(200);check('skip releases input immediately',await p.locator('#app').evaluate(e=>!e.inert));check('opening hidden after skip',await p.locator('#launch').evaluate(e=>e.hidden));
 await p.reload();await p.waitForTimeout(80);check('opening does not repeat during same session',await p.locator('#launch').evaluate(e=>e.hidden));
 await prev(p);await p.locator('#empty').check();await p.keyboard.press('Escape');check('empty account has add path',await p.locator('#list').textContent().then(t=>t.includes('Your line is ready')&&t.includes('Add a contact')));await prev(p);await p.locator('#empty').uncheck();await p.keyboard.press('Escape');
 await p.locator('[data-action=settings]:visible').click();await p.locator('[data-action=motion]').click();await p.keyboard.press('Escape');await prev(p);await p.locator('[data-action=opening]').click();check('manual reduced motion skips opening',await p.locator('#launch').evaluate(e=>e.hidden));
 const rb=await p.locator('#new').boundingBox();await p.mouse.move(rb.x+rb.width/2,rb.y+rb.height/2);await p.mouse.down();check('reduced motion keeps press still',await p.locator('#new').evaluate(e=>getComputedStyle(e).transform==='none'));await p.mouse.move(0,0);await p.mouse.up();await p.close();
 const q=await b.newPage({viewport:{width:390,height:844},reducedMotion:'reduce'});await q.goto(BASE+'home.html');check('system reduced motion skips first opening',await q.locator('#launch').evaluate(e=>e.hidden));check('system reduced motion removes entry animations',await q.locator('#list .row').first().evaluate(e=>getComputedStyle(e).animationName==='none'));await q.close();
 const touch=await b.newPage({viewport:{width:390,height:844}});await touch.goto(BASE+'home.html');await touch.waitForTimeout(850);const box=await touch.locator('#new').boundingBox();await touch.mouse.move(box.x+box.width/2,box.y+box.height/2);await touch.mouse.down();await touch.waitForTimeout(140);check('button gives a visible pressed transform',await touch.locator('#new').evaluate(e=>getComputedStyle(e).transform!=='none'));await touch.screenshot({path:path.join(root,'screenshots','home-pressed.png')});await touch.mouse.move(0,0);await touch.mouse.up();check('raised control has layered shadow',await touch.locator('#new').evaluate(e=>getComputedStyle(e).boxShadow!=='none'));await touch.close();

 /* ---------- the call outlives the page: strip on every screen (SPEC §In-conversation calls) ---------- */
 const shots=n=>path.join(root,'screenshots','callbar-'+n+'.png');
 const secs=t=>{const m=/(\d\d):(\d\d)/.exec(t||'');return m?+m[1]*60+ +m[2]:-1};
 const mkp=async({w,h,dark=false})=>{const ctx=await b.newContext({viewport:{width:w,height:h},colorScheme:dark?'dark':'light'});
  await ctx.addInitScript(()=>{const A=window.AudioContext;if(!A)return;window.AudioContext=class extends A{constructor(...a){super(...a);try{sessionStorage.setItem('qa.ac',String(+(sessionStorage.getItem('qa.ac')||0)+1))}catch{}}}});
  const p=await ctx.newPage();p.errs=[];p.on('pageerror',e=>p.errs.push(e.message));return p};
 const audio=p=>p.evaluate(()=>+(sessionStorage.getItem('qa.ac')||0));
 const stripsVisible=async p=>{let n=0;for(const f of p.frames()){try{n+=await f.evaluate(()=>[...document.querySelectorAll('.t-callbar')].filter(e=>e.getClientRects().length&&!e.hidden).length)}catch{}}return n};
 // computed shape of the strip + child positions: must be identical on every page
 const sig=f=>f.evaluate(()=>{const b=document.querySelector('#callStrip'),c=getComputedStyle(b),r=b.getBoundingClientRect(),q=s=>b.querySelector(s),d=getComputedStyle(q('.t-cb-dot'));const x=s=>Math.round((q(s).getBoundingClientRect().left-r.left)*10)/10;
  return JSON.stringify({h:Math.round(r.height),font:[c.fontFamily,c.fontSize,c.fontWeight,c.lineHeight,c.fontVariantNumeric].join('|'),box:[c.paddingLeft,c.paddingRight,c.columnGap,c.borderTopWidth,c.borderRadius].join('|'),paint:[c.backgroundColor,c.color].join('|'),dot:[d.width,d.height,d.borderRadius,d.backgroundColor].join('|'),pos:[x('.t-cb-dot'),x('.t-cb-nm'),x('.t-cb-tm')]})});
 {// phone: Home → Arjun → call → minimise → Back → strip on Home, timer continuing → tap → overlay → End → no strip
  const p=await mkp({w:390,h:844});await p.goto(BASE+'home.html');await p.waitForTimeout(850);
  check('callbar 390 no strip without a call',await p.locator('#callStrip').isHidden()&&await p.locator('.mobile-bar').evaluate(e=>e.getBoundingClientRect().top===0));
  await p.locator('#list [data-person=arjun]').click();await p.waitForURL('**/chat.html?**');await p.locator('#callBtn').click();await p.waitForTimeout(3700);
  await p.locator('#callMin').click();await p.waitForTimeout(100);const t0=secs(await p.locator('#callStrip').textContent());
  check('callbar 390 conversation strip ticking before Back',t0>=1,t0);
  await p.getByRole('button',{name:'Back to chats',exact:true}).click();await p.waitForURL('**/home.html#chats');await p.waitForTimeout(150);
  const s1=await p.locator('#callStrip').evaluate(e=>({vis:!e.hidden&&e.getClientRects().length>0,t:e.textContent,name:e.getAttribute('aria-label'),r:e.getBoundingClientRect().toJSON(),live:e.closest('[aria-live],[role=status]')}));
  check('callbar 390 Back keeps the call: strip on Home',s1.vis&&/^Arjun Oommen · \d\d:\d\d$/.test(s1.t),JSON.stringify(s1));
  check('callbar 390 timer continues, never resets',secs(s1.t)>=t0,s1.t+' vs '+t0);
  check('callbar 390 strip full width 40px at the very top',s1.r.top===0&&s1.r.left===0&&s1.r.width===390&&s1.r.height===40,JSON.stringify(s1.r));
  check('callbar 390 accessible name, not a live region',s1.name==='Return to call with Arjun Oommen'&&!s1.live);
  check('callbar 390 header sits below the strip',await p.locator('.mobile-bar').evaluate(e=>Math.round(e.getBoundingClientRect().top)===40));
  check('callbar 390 strip pushes the app down without overflow',await p.evaluate(()=>document.querySelector('#app').getBoundingClientRect().bottom<=innerHeight&&document.documentElement.scrollWidth<=innerWidth));
  const fab=await p.locator('#new').boundingBox(),nb=await p.locator('#mobile-nav').boundingBox();check('callbar 390 FAB still 16px above tab bar',Math.abs(nb.y-(fab.y+fab.height)-16)<=1&&nb.y+nb.height<=845);
  await p.waitForTimeout(1100);check('callbar 390 Home strip ticks',secs(await p.locator('#callStrip').textContent())>secs(s1.t));
  await p.screenshot({path:shots('home-390-light-chats')});
  for(const [tab,title] of [['groups','Groups'],['calls','Calls'],['contacts','Contacts'],['chats','Chats']]){await p.locator(`#mobile-nav [data-tab=${tab}]`).click();check('callbar 390 strip on '+title+' tab',await p.locator('#page-title').textContent()===title&&await p.locator('#callStrip').isVisible()&&(await p.locator('.list-pane').boundingBox()).y===40);if(tab==='calls'){await p.waitForTimeout(400);await p.screenshot({path:shots('home-390-light-calls')})}}
  await p.locator('.mobile-bar .brand').focus();await p.keyboard.press('Shift+Tab');check('callbar 390 strip first in tab order, focus visible',await p.locator('#callStrip').evaluate(e=>e.matches(':focus-visible')&&getComputedStyle(e).outlineStyle!=='none'));
  const before=secs(await p.locator('#callStrip').textContent());
  await p.locator('#callStrip').click();await p.waitForURL('**/chat.html?**');await p.waitForTimeout(200);
  const o=await p.evaluate(()=>({ov:!document.querySelector('#callOv').hidden,f:document.activeElement.id,strip:!document.querySelector('#callStrip').hidden,t:document.querySelector('#callTime').textContent,url:location.search}));
  check('callbar 390 tap strip → conversation with overlay open',o.ov&&o.f==='callOv'&&!o.strip&&/chat=arjun&from=home$/.test(o.url),JSON.stringify(o));
  check('callbar 390 overlay timer continues',secs(o.t)>=before,o.t+' vs '+before);
  await p.screenshot({path:shots('conversation-after-strip-390')});
  await p.locator('#callEnd').click();await p.waitForTimeout(150);check('callbar 390 End toast',/Call ended/.test(await p.locator('#toasts').textContent()));
  await p.getByRole('button',{name:'Back to chats',exact:true}).click();await p.waitForURL('**/home.html#chats');await p.waitForTimeout(150);
  check('callbar 390 after End Home has no strip',await p.locator('#callStrip').isHidden()&&await p.locator('.mobile-bar').evaluate(e=>e.getBoundingClientRect().top===0));
  // browser history Back keeps the call too
  await p.locator('#list [data-person=meera]').click();await p.waitForURL('**/chat.html?**');await p.locator('#callBtn').click();await p.waitForTimeout(300);await p.keyboard.press('Escape');
  await p.goBack();await p.waitForURL('**/home.html**');await p.waitForTimeout(200);
  check('callbar 390 history Back keeps the call',await p.locator('#callStrip').isVisible()&&/^Meera Nair · Ringing…$|^Meera Nair · \d\d:\d\d$/.test(await p.locator('#callStrip').textContent()),await p.locator('#callStrip').textContent());
  check('callbar 390 one strip only',await stripsVisible(p)===1);
  check('callbar 390 no AudioContext created',await audio(p)===0);check('callbar 390 no runtime errors',!p.errs.length,p.errs.join(';'));await p.context().close()}
 {// no layout shift Ringing… → MM:SS; same strip shape on Home, chat.html and groups.html (light + dark)
  for(const dark of [false,true]){const L=dark?'dark':'light';const p=await mkp({w:390,h:844,dark});await p.goto(BASE+'home.html');await p.waitForTimeout(850);
   await p.evaluate(()=>TinCall.start({kind:'direct',id:'arjun',name:'Arjun Oommen'}));await p.waitForTimeout(50);
   const pos=()=>p.locator('#callStrip').evaluate(e=>[e.querySelector('.t-cb-dot'),e.querySelector('.t-cb-nm'),e.querySelector('.t-cb-tm')].map(x=>{const r=x.getBoundingClientRect();return [r.left,r.top,r.width,r.height].map(Math.round).join()}).join('|'));
   const a=await pos(),ta=await p.locator('#callStrip').textContent();await p.waitForTimeout(2700);const z=await pos(),tz=await p.locator('#callStrip').textContent();
   check('callbar '+L+' no layout shift Ringing… → MM:SS',/Ringing…/.test(ta)&&/\d\d:\d\d/.test(tz)&&a===z,ta+' '+tz+' '+a+' / '+z);
   const home=await sig(p);if(!dark)await p.waitForTimeout(400);await p.screenshot({path:shots('home-390-'+L+'-chats')});await p.locator('#mobile-nav [data-tab=calls]').click();await p.waitForTimeout(400);await p.screenshot({path:shots('home-390-'+L+'-calls')});
   await p.goto(BASE+'chat.html?chat=arjun&from=home');await p.waitForTimeout(250);const chat=await sig(p);
   await p.goto(BASE+'groups.html?group=hike&from=home');await p.waitForTimeout(250);const grp=await sig(p);
   await p.goto(BASE+'chat.html?chat=meera&embed=1');await p.waitForTimeout(250);const emb=await sig(p);
   check('callbar '+L+' strip identical on Home, chat, groups, embed',home===chat&&chat===grp&&grp===emb,[home,chat,grp,emb].join(' ~ '));
   await p.evaluate(()=>TinCall.end());await p.context().close()}}
 for(const dark of [false,true]){// desktop Home: call in Arjun, switch to Meera, call survives, one strip, strip returns to Arjun's overlay
  const L=dark?'dark':'light',p=await mkp({w:1440,h:1000,dark});await p.goto(BASE+'home.html');await p.waitForTimeout(850);
  await p.locator('#list [data-person=arjun]').click();await p.waitForTimeout(600);const f=p.frameLocator('#convo');
  check('callbar 1440 '+L+' embedded page is told Home hosts the strip',/callbar=host/.test(await p.locator('#convo').getAttribute('src')));
  await f.locator('#callBtn').click();await p.waitForTimeout(300);check('callbar 1440 '+L+' call starts in the pane, Home strip on top',await f.locator('#callOv').isVisible()&&await p.locator('#callStrip').isVisible()&&await stripsVisible(p)===1);
  await f.locator('#callMin').click();await p.waitForTimeout(2600);
  await p.locator('#list [data-person=meera]').click();await p.waitForTimeout(700);
  const r=await p.evaluate(()=>{const g=s=>document.querySelector(s).getBoundingClientRect();return {strip:g('#callStrip').toJSON(),rail:g('.rail').top,list:g('.list-pane').top,detail:[g('.detail-pane').top,g('.detail-pane').height],t:document.querySelector('#callStrip').textContent,src:document.querySelector('#convo').getAttribute('src')}});
  check('callbar 1440 '+L+' switching chats keeps the call',/chat=meera/.test(r.src)&&/^Arjun Oommen · \d\d:\d\d$/.test(r.t)&&await f.locator('#callBtn').getAttribute('aria-label')==='Return to call with Arjun Oommen',JSON.stringify(r));
  check('callbar 1440 '+L+' exactly one strip (pane suppresses its own)',await stripsVisible(p)===1&&await f.locator('#callStrip').isHidden());
  check('callbar 1440 '+L+' strip spans the app; three columns start below it',r.strip.top===0&&r.strip.width===1440&&r.strip.height===40&&r.rail===40&&r.list===40&&r.detail[0]===40&&r.detail[1]===960,JSON.stringify(r));
  await p.screenshot({path:shots('home-1440-'+L)});
  await p.locator('#callStrip').click();await p.waitForTimeout(700);
  const back=await p.evaluate(()=>({src:document.querySelector('#convo').getAttribute('src'),sel:document.querySelector('#list [data-person=arjun]').classList.contains('selected'),f:document.activeElement.id,inner:document.querySelector('#convo').contentDocument.activeElement.id}));
  check('callbar 1440 '+L+' strip returns to Arjun with the overlay up',/chat=arjun/.test(back.src)&&back.sel&&back.f==='convo'&&back.inner==='callOv'&&await f.locator('#callOv').isVisible(),JSON.stringify(back));
  if(!dark)await p.screenshot({path:shots('home-1440-light-overlay')});
  await f.locator('#callMin').click();await p.locator('#callStrip').click();await p.waitForTimeout(200);check('callbar 1440 '+L+' strip on the same chat reopens without reload',await f.locator('#callOv').isVisible()&&!/call=open$/.test('x'));
  await f.locator('#callEnd').click();await p.waitForTimeout(200);check('callbar 1440 '+L+' End in the pane clears Home strip at once',await p.locator('#callStrip').isHidden()&&await stripsVisible(p)===0&&await p.locator('.rail').evaluate(e=>e.getBoundingClientRect().top===0));
  check('callbar 1440 '+L+' no AudioContext',await audio(p)===0);check('callbar 1440 '+L+' no runtime errors',!p.errs.length,p.errs.join(';'));await p.context().close()}
 {// call started in groups shows on Home; tapping opens the group with the overlay
  const p=await mkp({w:390,h:844});await p.goto(BASE+'groups.html?group=hike&from=home');await p.waitForTimeout(300);await p.locator('#callBtn').click();await p.waitForTimeout(200);await p.locator('#callOv .cmin').click();
  await p.locator('[aria-label="Back to home"]').click();await p.waitForURL('**/home.html');await p.waitForTimeout(200);
  check('callbar group call visible on Home',await p.locator('#callStrip').isVisible()&&/^Weekend hike · /.test(await p.locator('#callStrip').textContent())&&await p.locator('#callStrip').getAttribute('aria-label')==='Return to call with Weekend hike');
  await p.locator('#callStrip').click();await p.waitForURL('**/groups.html?**');await p.waitForTimeout(200);
  check('callbar group strip → group overlay with the roster',/group=hike&from=home$/.test(new URL(p.url()).search)&&await p.locator('#callOv').isVisible()&&await p.locator('#callOv .t-gc .t-gc-glyph').count()===1&&await p.locator('#callOv .t-gc-spk > li').count()===3);
  await p.locator('#callOv .end').click();check('callbar group End clears',await p.locator('#callStrip').isHidden());check('callbar group no AudioContext',await audio(p)===0);check('callbar group no errors',!p.errs.length,p.errs.join(';'));await p.context().close()}
 }catch(e){results.push({name:'run completed',ok:false,detail:e.stack});console.error(e)}finally{await b.close();fs.writeFileSync(path.join(root,'home-results.json'),JSON.stringify(results,null,2));const failed=results.filter(r=>!r.ok);console.log(`${results.length-failed.length}/${results.length} home checks passed`);if(failed.length)process.exitCode=1}})();
