const L=require('./lib');
(async()=>{const b=await L.launch();const p=await L.page(b);let bad=[];const seen=new Set();
for(const n of ['index','home','review','calls','chat','groups','privacy','devices']){await p.goto(L.BASE+n+'.html');await p.waitForTimeout(300);
 const hs=await p.$$eval('a[href]',e=>e.map(a=>a.href));
 for(const h of hs){const u=h.split('#')[0];const k=n+'→'+u;if(seen.has(k)||!u.startsWith('http://127.0.0.1'))continue;seen.add(k);const r=await p.request.get(u);if(!r.ok())bad.push(`${n} → ${h} (${r.status()})`)}}
console.log(seen.size+' links checked; broken:',bad.length?'\n'+bad.join('\n'):'none');await b.close();if(bad.length)process.exitCode=1})();
