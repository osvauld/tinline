const L=require('./lib');
const pages=['index','calls','chat','groups','privacy'];
(async()=>{const b=await L.launch();const out=[];
for(const n of pages)for(const [vn,w,h,m] of [['desktop',1440,1000,false],['mobile',390,844,true]])for(const dark of [false,true]){
 if(dark&&!['calls','privacy'].includes(n)&&vn==='mobile'&&false)continue;
 const p=await L.page(b,{w,h,dark,mobile:m});await p.goto(L.BASE+n+'.html');await p.waitForTimeout(800);
 const o=await L.overflow(p);
 await p.screenshot({path:`screenshots/${n}-${vn}-${dark?'dark':'light'}.png`,fullPage:false});
 out.push(`${n} ${vn} ${dark?'dark':'light'} overflow=${JSON.stringify(o)} errs=${p.errs.length} fails=${p.fails.length}`+(p.errs.length?' '+p.errs.join('|'):''));
 await p.context().close();}
console.log(out.join('\n'));await b.close()})();
