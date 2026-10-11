/* Tinline refined studies — the one synthesized call-cue set. SPEC §In-conversation calls, "Sound".

   PAT and Snd are calls.html's own tone generator, moved here verbatim (same frequencies, timing,
   labels) so calls.html, chat.html and groups.html sound identical. Two additions, neither changes a
   pattern: Snd.onchange (calls.html wires its caption to it) and an optional gain factor on Snd.start
   (used only for the "soft" pulse when someone joins or leaves a group call).

   Loaded by: calls.html (uses PAT/Snd directly, keeps its own switch, captions and cleanup),
              chat.html + groups.html (use the in-conversation cue director below).

   API  TinAudio.PAT · Snd                       raw generator (calls.html)
        TinAudio.follow(TinCall)                 in-conversation director: plays the overlay cues from call state
        TinAudio.arm()                           call from a user gesture (call button, strip, End, Quiet switch)
        TinAudio.ending()                        call from the End click, right before TinCall.end()
        TinAudio.quiet() · setQuiet(bool) · onQuiet(fn)   Quiet mode, on unless the person turned it off
        TinAudio.played                          cue names this document played, e.g. ["back","connected","ended"]

   Cue rules (overlay; it is always the caller's own view, so the incoming `ring` never plays here):
     direct  `back` loops while "Ringing…", `connected` once on connect, `ended` once on End.
     group   never `ring`/`back` (a group call rings nobody, U21). `pulse` once on joining, `connected`
             once you are in the roster, soft `pulse` when someone else joins or leaves, `ended` on leaving.
   Nothing makes an AudioContext until arm() runs inside a gesture, and Quiet mode never makes one at all.
   Hidden tab stops the current sound, pagehide closes the context — the calls.html discipline. */
(function () {
'use strict';

/* ---------- generator: verbatim from calls.html ---------- */
var PAT={
 ring:{label:'Ringtone',text:'soft rising four-note chime, repeating about every 3 s',period:3200,notes:[[784,0,1,.5,.02],[988,.18,1,.45,.02],[1175,.36,1,.4,.02],[1568,.7,1.4,.3,.02]]},
 back:{label:'Ringback',text:'calm two-tone hum, about 1 s on and 3 s off',period:4000,notes:[[392,0,1.2,.4,.18],[494,0,1.2,.35,.18]]},
 connected:{label:'Connected cue',text:'two quick rising notes',period:0,notes:[[523,0,.5,.45,.02],[784,.12,.7,.45,.02]]},
 ended:{label:'Ended cue',text:'two falling notes',period:0,notes:[[784,0,.45,.4,.02],[523,.12,.7,.4,.02]]},
 failed:{label:'Couldn’t connect cue',text:'two low falling notes',period:0,notes:[[247,0,.5,.45,.02],[196,.24,.8,.45,.02]]},
 pulse:{label:'Reconnecting cue',text:'one soft low note, once',period:0,notes:[[330,0,.6,.35,.02]]}
};
var Snd={ctx:null,vol:.5,cur:null,onchange:null,
 ensure:function(){
  if(!this.ctx){var C=window.AudioContext||window.webkitAudioContext;if(!C)return false;try{this.ctx=new C()}catch(e){return false}}
  if(this.ctx.state==='suspended'){this.ctx.resume()}
  return true;
 },
 start:function(kind,max,src,gain){
  this.stop();
  if(!this.ensure())return false;
  var c=this.ctx,g=c.createGain(),lp=c.createBiquadFilter();
  lp.type='lowpass';lp.frequency.value=3200;
  gain=gain||1;
  g.gain.value=this.vol*.25*gain;
  lp.connect(g);g.connect(c.destination);
  var s={kind:kind,src:src,g:g,lp:lp,oscs:[],timers:[],stopped:false,gain:gain};
  this.cur=s;
  var self=this,n=0;
  (function cyc(){
   if(s.stopped)return;
   var p=PAT[kind],t=c.currentTime+.03;
   p.notes.forEach(function(x){self.tone(s,x[0],t+x[1],x[2],x[3],x[4])});
   n++;
   var wait=p.period||Math.max.apply(null,p.notes.map(function(x){return(x[1]+x[2])*1000}))+80;
   s.timers.push(setTimeout(function(){
    if(s.stopped)return;
    if(p.period&&n<max){cyc()}else{self.stop(s,true)}
   },p.period&&n<max?p.period:wait));
  })();
  return true;
 },
 tone:function(s,f,t0,dur,peak,atk){
  var c=this.ctx;
  [[f,1],[f*2,.18]].forEach(function(pr){
   var o=c.createOscillator(),e=c.createGain();
   o.type='sine';o.frequency.value=pr[0];
   e.gain.setValueAtTime(0,t0);
   e.gain.linearRampToValueAtTime(peak*pr[1],t0+atk);
   e.gain.exponentialRampToValueAtTime(.0001,t0+dur);
   o.connect(e);e.connect(s.lp);
   o.onended=function(){try{e.disconnect()}catch(x){}};
   o.start(t0);o.stop(t0+dur+.05);
   s.oscs.push(o);
  });
 },
 stop:function(s,natural){
  s=s||this.cur;if(!s||s.stopped)return;
  s.stopped=true;s.timers.forEach(clearTimeout);
  var c=this.ctx,now=c.currentTime;
  s.g.gain.cancelScheduledValues(now);
  s.g.gain.setTargetAtTime(0,now,.012);
  s.oscs.forEach(function(o){try{o.stop(now+.08)}catch(x){}});
  setTimeout(function(){try{s.g.disconnect();s.lp.disconnect()}catch(x){}},200);
  if(this.cur===s)this.cur=null;
  if(this.onchange)this.onchange(natural?'ended':'stopped',s);
 },
 setVol:function(v){this.vol=v;if(this.cur){this.cur.g.gain.setTargetAtTime(v*.25*(this.cur.gain||1),this.ctx.currentTime,.02)}},
 close:function(){this.stop();if(this.ctx){try{this.ctx.close()}catch(e){}this.ctx=null}}
};

/* ---------- Quiet mode (shared by chat.html + groups.html; on unless turned off) ---------- */
var QKEY='tinline.callQuiet',qsubs=[];
function quiet(){try{return localStorage.getItem(QKEY)!=='0'}catch(e){return true}}
// Only ever called from the Quiet switch (a gesture), so turning sound on may unlock audio here.
function setQuiet(on){
 try{localStorage.setItem(QKEY,on?'1':'0')}catch(e){}
 if(!on)arm();
 applyQuiet();
}
function applyQuiet(){
 var on=quiet();
 if(on){Snd.stop()}else if(armed){resume()}
 qsubs.forEach(function(f){try{f(on)}catch(e){console.error(e)}});
}
function onQuiet(fn){qsubs.push(fn);return function(){qsubs=qsubs.filter(function(x){return x!==fn})}}

/* ---------- in-conversation cue director ---------- */
var SOFT=.4;          // the "someone joined/left" pulse: same note, under half the level
var TC=null,armed=false,prev=null,endPending=false,played=[];
function arm(){armed=true;if(!quiet())Snd.ensure()}
function play(kind,loop,soft){
 if(!armed||quiet())return false;
 if(!Snd.start(kind,loop?Infinity:1,'call',soft?SOFT:1))return false;
 played.push(kind+(soft?':soft':''));return true;
}
function others(c){return c&&c.kind==='group'&&c.people?c.people.filter(function(p){return !p.me}).map(function(p){return p.id}).join(','):''}
function snap(c){return c?{ringAt:c.ringAt,kind:c.kind,connected:c.connected,ids:others(c)}:null}
function step(c){
 var cur=snap(c),p=prev;prev=cur;
 if(!cur){
  if(p){Snd.stop();if(endPending)play('ended')}
  endPending=false;return;
 }
 if(!p||p.ringAt!==cur.ringAt){                 // a call this document has not seen yet
  if(cur.kind==='direct'&&!cur.connected)play('back',true);
  else if(cur.kind==='group')play('pulse');
  return;
 }
 if(!p.connected&&cur.connected){Snd.stop();play('connected');return}
 if(cur.kind==='group'&&cur.connected&&p.ids!==cur.ids)play('pulse',false,true);
}
// Bring sound back to where the call is (Quiet turned off, tab shown again): only ringback loops.
function resume(){var c=TC&&TC.get();if(c&&c.kind==='direct'&&!c.connected&&!(Snd.cur&&Snd.cur.kind==='back'))play('back',true)}
function follow(tc){
 if(TC)return;TC=tc;prev=snap(TC.get());
 TC.subscribe(step);TC.onTick(step);
 document.addEventListener('visibilitychange',function(){if(document.hidden)Snd.stop();else if(armed&&!quiet())resume()});
 window.addEventListener('pagehide',function(){Snd.close();armed=false});
 window.addEventListener('storage',function(e){if(e.key===QKEY||e.key===null)applyQuiet()});
}
function ending(){arm();endPending=true}

window.TinAudio={PAT:PAT,Snd:Snd,follow:follow,arm:arm,ending:ending,quiet:quiet,setQuiet:setQuiet,onQuiet:onQuiet,played:played};
})();
