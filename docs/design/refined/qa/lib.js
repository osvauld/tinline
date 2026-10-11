const {chromium}=require(process.env.PLAYWRIGHT_PATH || '/home/abe/project/sveditorjs/node_modules/playwright');
const BASE=process.env.DESIGN_BASE_URL || 'http://127.0.0.1:8768/refined/';
const IGN=/support\.js|fonts\.g|favicon/;
async function launch(){return chromium.launch({executablePath:process.env.CHROMIUM_PATH || '/usr/sbin/chromium',args:['--no-sandbox']});}
// New page with console/pageerror/requestfailed capture and AudioContext instrumentation.
async function page(b,{w=1440,h=1000,dark=false,mobile=false}={}){
 const ctx=await b.newContext({viewport:{width:w,height:h},colorScheme:dark?'dark':'light',hasTouch:mobile,isMobile:mobile,deviceScaleFactor:1});
 await ctx.addInitScript(()=>{
  const L=window.__aud={osc:[],started:0,stopped:0,ctxs:[],log:[]};
  const AC=window.AudioContext||window.webkitAudioContext; if(!AC)return;
  const P=AC.prototype, co=P.createOscillator;
  P.createOscillator=function(){const o=co.apply(this,arguments);const rec={t:this.currentTime,started:false,stopped:false,ended:false};L.osc.push(rec);
   const s=o.start,t=o.stop;o.start=function(){rec.started=true;L.started++;L.log.push('start');return s.apply(this,arguments)};
   o.stop=function(){rec.stopped=true;L.stopped++;L.log.push('stop');return t.apply(this,arguments)};
   o.addEventListener('ended',()=>{rec.ended=true;L.log.push('ended')});return o};
  const wrap=function(N){return class extends N{constructor(){super(...arguments);L.ctxs.push(this)}}};
  window.AudioContext=wrap(AC); if(window.webkitAudioContext)window.webkitAudioContext=window.AudioContext;
 });
 const p=await ctx.newPage(); p.errs=[];p.fails=[];
 p.on('console',m=>{if(m.type()==='error'&&!IGN.test(m.text()+(m.location().url||'')))p.errs.push(m.text()+' @'+m.location().url)});
 p.on('pageerror',e=>p.errs.push('pageerror: '+e.message));
 p.on('requestfailed',r=>{if(!IGN.test(r.url()))p.fails.push(r.url())});
 p.on('response',r=>{if(r.status()>=400&&!IGN.test(r.url()))p.fails.push(r.status()+' '+r.url())});
 return p;}
const aud=p=>p.evaluate(()=>{const a=window.__aud||{};return {started:a.started,stopped:a.stopped,live:(a.osc||[]).filter(o=>o.started&&!o.stopped&&!o.ended).length,ctxStates:(a.ctxs||[]).map(c=>c.state),log:(a.log||[]).join(',')}});
const overflow=p=>p.evaluate(()=>({sw:document.documentElement.scrollWidth,cw:document.documentElement.clientWidth,bw:document.body.scrollWidth}));
module.exports={launch,page,aud,overflow,BASE};
