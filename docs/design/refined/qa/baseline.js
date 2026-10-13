const {chromium}=require('/home/abe/project/sveditorjs/node_modules/playwright');
(async()=>{
const b=await chromium.launch({executablePath:'/usr/sbin/chromium',args:['--no-sandbox']});
const p=await b.newPage({viewport:{width:1440,height:1000}});
const errs=[];p.on('console',m=>m.type()==='error'&&errs.push(m.text()));p.on('requestfailed',r=>errs.push('reqfail '+r.url()));
for(const [n,u] of [['AndroidCalls','../../AndroidCalls.dc.html'],['AndroidPrivacy','../../groups/AndroidPrivacy.dc.html']]){
 const url=new URL(u,'http://127.0.0.1:8768/refined/qa/').href.replace('/refined/qa/../../','/');
 const r=await p.goto('http://127.0.0.1:8768/'+(n==='AndroidCalls'?'AndroidCalls.dc.html':'groups/AndroidPrivacy.dc.html'),{timeout:15000}).catch(e=>e.message);
 await p.waitForTimeout(1500);
 console.log(n,r&&r.status?r.status():r, (await p.evaluate(()=>document.body.innerText.length)));
 await p.screenshot({path:`screenshots/baseline-${n}.png`});
}
console.log(errs.slice(0,6));await b.close()})();
