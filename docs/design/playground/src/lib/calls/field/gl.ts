/* WebGL1 renderer for the cloud field: one full-screen triangle, one fragment shader. Falls back to
   CSS radial gradients (same lobes, no warp) when WebGL is unavailable or the shader fails. */
import { labToRgb } from './model';

const VS = 'attribute vec2 a;void main(){gl_Position=vec4(a,0.,1.);}';
const FS = `
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
uniform vec2 uRes; uniform float uShort, uTau, uWarp, uCap, uWs, uNr;
uniform vec3 uBg; uniform vec4 uScr[2]; uniform float uScrK, uGain; uniform vec2 uC[5]; uniform float uSig[5]; uniform float uA[5]; uniform vec3 uLab[5];
vec2 h23(vec3 p){ p=fract(p*vec3(.1031,.1030,.0973)); p+=dot(p,p.yzx+33.33); return fract((p.xx+p.yz)*p.zy)*2.-1.; }
vec2 vn(vec3 p){ vec3 i=floor(p), f=fract(p); vec3 u=f*f*f*(f*(f*6.-15.)+10.);
  vec2 a=mix(mix(h23(i),h23(i+vec3(1,0,0)),u.x),mix(h23(i+vec3(0,1,0)),h23(i+vec3(1,1,0)),u.x),u.y);
  vec2 b=mix(mix(h23(i+vec3(0,0,1)),h23(i+vec3(1,0,1)),u.x),mix(h23(i+vec3(0,1,1)),h23(i+vec3(1,1,1)),u.x),u.y);
  return mix(a,b,u.z); }
vec3 ok2lin(vec3 c){ float l=c.x+.3963377774*c.y+.2158037573*c.z, m=c.x-.1055613458*c.y-.0638541728*c.z, s=c.x-.0894841775*c.y-1.2914855480*c.z; l=l*l*l; m=m*m*m; s=s*s*s;
  return vec3(4.0767416621*l-3.3077115913*m+.2309699292*s,-1.2684380046*l+2.6097574011*m-.3413193965*s,-.0041960863*l-.7034186147*m+1.7076147010*s); }
vec3 lin2s(vec3 c){ c=clamp(c,0.,1.); return mix(12.92*c,1.055*pow(c,vec3(1./2.4))-.055,step(.0031308,c)); }
void main(){
  vec2 p=vec2(gl_FragCoord.x-.5*uRes.x,.5*uRes.y-gl_FragCoord.y)/uShort;
  mat2 R=mat2(.8,.6,-.6,.8); float z=uTau*uNr;
  vec2 w=vn(vec3(p*uWs,z))*.55; vec2 p2=R*p;
  w+=vn(vec3(p2*uWs*2.07+7.3,z*1.3+3.))*.30; vec2 p3=R*p2;
  w+=vn(vec3(p3*uWs*4.13+13.1,z*1.7+9.))*.15;
  vec2 q=p+uWarp*w;
  // Coverage S sums the lobes, colour is their weighted mean. Light mode restores the mean's chroma (a
  // pastel mid-mix stays fresh); dark mode mostly doesn't, because a re-saturated teal/amber mean is olive.
  float S=0., W=0.; vec3 ab=vec3(0.); float cs=0.;
  for(int i=0;i<5;i++){ vec2 d=q-uC[i]; float a=uA[i]*exp(-dot(d,d)/(2.*uSig[i]*uSig[i])); float w=a+1e-9; S+=a; W+=w; ab+=w*uLab[i]; cs+=w*length(uLab[i].yz); }
  vec3 lab=uBg;
  if(S>1e-5){ vec3 m=ab/W; cs/=W; float ct0=cs; float cm=length(m.yz); float ct=mix(cm,ct0,uGain>1.01?.25:1.); if(cm>1e-4) m.yz*=ct/cm;
    lab=mix(uBg,m,uCap*(1.-exp(-S))); }
  // calm patches behind the text block and the controls (flat inside, soft edge): the clouds can carry
  // brighter colour everywhere else while every label keeps its contrast
  float sc=0.;
  for(int i=0;i<2;i++){ vec2 d=(p-uScr[i].xy)/uScr[i].zw; sc=max(sc,1.-smoothstep(.0,1.,length(d))); }
  lab=mix(lab,uBg,uScrK*sc);
  lab.yz*=uGain;   // dark mode: the mix with the background drains chroma; give it back so colour reads as colour
  vec3 rgb=lin2s(ok2lin(lab));
  float n=fract(52.9829189*fract(dot(gl_FragCoord.xy,vec2(.06711056,.00583715))));
  rgb+=(n-.5)/255.;
  gl_FragColor=vec4(rgb,1.);
}`;

/** Calm patches in short-side-normalised space: centre x,y and radii. k = strength 0..1. */
export interface Scrim { k: number; at: [number, number, number, number][] }
export interface Frame { tau: number; warp: number; cap: number; bgLab: number[]; lobes: { x: number; y: number; sigma: number; alpha: number; lab: number[] }[]; scrim?: Scrim; gain?: number }
export interface Params { warpScale: number; noiseRate: number }
export interface Renderer { kind: 'webgl' | 'css'; draw(f: Frame, P: Params): void; resize(): void }

export function createRenderer(canvas: HTMLCanvasElement, host: HTMLElement): Renderer {
  const gl = initGL(canvas);
  if (!gl) return cssRenderer(canvas, host);
  const { ctx, U } = gl;
  const cC = new Float32Array(10), cS = new Float32Array(5), cA = new Float32Array(5), cL = new Float32Array(15), scr = new Float32Array(8);
  let cw = 1, ch = 1;
  const resize = () => {
    const w = host.clientWidth, h = host.clientHeight;
    let s = Math.min(window.devicePixelRatio || 1, 1.5);
    if (w * h * s * s > 700_000) s = Math.sqrt(700_000 / (w * h));   // keep phone GPUs comfortable
    cw = Math.max(1, Math.round(w * s)); ch = Math.max(1, Math.round(h * s));
    if (canvas.width !== cw || canvas.height !== ch) { canvas.width = cw; canvas.height = ch; }
  };
  resize();
  return {
    kind: 'webgl', resize,
    draw(f, P) {
      ctx.viewport(0, 0, cw, ch);
      ctx.uniform2f(U.uRes, cw, ch); ctx.uniform1f(U.uShort, Math.min(cw, ch)); ctx.uniform1f(U.uTau, f.tau);
      ctx.uniform1f(U.uWarp, f.warp); ctx.uniform1f(U.uCap, f.cap); ctx.uniform1f(U.uWs, P.warpScale); ctx.uniform1f(U.uNr, P.noiseRate);
      ctx.uniform3f(U.uBg, f.bgLab[0], f.bgLab[1], f.bgLab[2]);
      const sc = f.scrim, a = sc?.at ?? [];
      scr.fill(0); a.slice(0, 2).forEach((r, i) => scr.set(r, 4 * i)); for (let i = a.length; i < 2; i++) scr.set([0, 0, 1, 1], 4 * i);
      ctx.uniform4fv(U['uScr[0]'], scr); ctx.uniform1f(U.uScrK, a.length ? sc!.k : 0); ctx.uniform1f(U.uGain, f.gain ?? 1);
      f.lobes.forEach((l, i) => { cC[2 * i] = l.x; cC[2 * i + 1] = l.y; cS[i] = l.sigma; cA[i] = l.alpha; cL.set(l.lab, 3 * i); });
      ctx.uniform2fv(U['uC[0]'], cC); ctx.uniform1fv(U['uSig[0]'], cS); ctx.uniform1fv(U['uA[0]'], cA); ctx.uniform3fv(U['uLab[0]'], cL);
      ctx.drawArrays(ctx.TRIANGLES, 0, 3);
    },
  };
}

function initGL(canvas: HTMLCanvasElement) {
  try {
    if (new URLSearchParams(location.search).has('nogl')) return null;
    const ctx = (canvas.getContext('webgl', { alpha: false, antialias: false, preserveDrawingBuffer: true }) ||
      canvas.getContext('experimental-webgl')) as WebGLRenderingContext | null;
    if (!ctx) return null;
    const sh = (t: number, s: string) => { const o = ctx.createShader(t)!; ctx.shaderSource(o, s); ctx.compileShader(o); if (!ctx.getShaderParameter(o, ctx.COMPILE_STATUS)) throw new Error(ctx.getShaderInfoLog(o) ?? 'shader'); return o; };
    const pr = ctx.createProgram()!;
    ctx.attachShader(pr, sh(ctx.VERTEX_SHADER, VS)); ctx.attachShader(pr, sh(ctx.FRAGMENT_SHADER, FS)); ctx.linkProgram(pr);
    if (!ctx.getProgramParameter(pr, ctx.LINK_STATUS)) throw new Error(ctx.getProgramInfoLog(pr) ?? 'link');
    ctx.useProgram(pr);
    const b = ctx.createBuffer(); ctx.bindBuffer(ctx.ARRAY_BUFFER, b);
    ctx.bufferData(ctx.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), ctx.STATIC_DRAW);
    const a = ctx.getAttribLocation(pr, 'a'); ctx.enableVertexAttribArray(a); ctx.vertexAttribPointer(a, 2, ctx.FLOAT, false, 0, 0);
    const U: Record<string, WebGLUniformLocation | null> = {};
    for (const n of ['uRes', 'uShort', 'uTau', 'uWarp', 'uCap', 'uWs', 'uNr', 'uBg', 'uScr[0]', 'uScrK', 'uGain', 'uC[0]', 'uSig[0]', 'uA[0]', 'uLab[0]']) U[n] = ctx.getUniformLocation(pr, n);
    return { ctx, U };
  } catch (e) { console.warn('WebGL unavailable, using CSS fallback:', (e as Error).message); return null; }
}

function cssRenderer(canvas: HTMLCanvasElement, host: HTMLElement): Renderer {
  canvas.style.display = 'none';
  const rgb = (c: number[], a: number) => `rgba(${c.map((x) => Math.round(x)).join(',')},${a.toFixed(3)})`;
  return {
    kind: 'css', resize() {},
    draw(f) {
      const W = host.clientWidth, H = host.clientHeight, sh = Math.min(W, H);
      // background colour = the theme background, so the gradients sit on it exactly like the shader's mix
      host.style.backgroundColor = rgb(labToRgb(f.bgLab), 1);
      const bg = labToRgb(f.bgLab);
      const scr = (f.scrim?.at ?? []).map((r) => `radial-gradient(ellipse ${Math.round(r[2] * sh)}px ${Math.round(r[3] * sh)}px at ${Math.round(W / 2 + r[0] * sh)}px ${Math.round(H / 2 + r[1] * sh)}px,${rgb(bg, f.scrim!.k)} 55%,${rgb(bg, 0)} 100%)`);
      host.style.backgroundImage = [...scr, ...f.lobes.map((l) => {
        const c = labToRgb(l.lab), a = f.cap * (1 - Math.exp(-l.alpha));
        return `radial-gradient(circle ${Math.round(l.sigma * sh * 2.2)}px at ${Math.round(W / 2 + l.x * sh)}px ${Math.round(H / 2 + l.y * sh)}px,${rgb(c, a)} 0,${rgb(c, a * .45)} 45%,${rgb(c, 0)} 100%)`;
      })].join(',');
    },
  };
}
