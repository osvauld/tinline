/* Svelte actions that turn shape.ts paths into DOM:
   use:shape       clip-path: path(...) on the element, plus an optional stroke overlay (a hairline that follows the same outline).
   use:shapeRing   for an absolutely positioned <svg> sitting around a box (found / flash / focus rings): every <path data-inset="n">
                   inside gets d = the shape inset by n px, so rings stay concentric with the element they surround.
   One ResizeObserver serves every target; work is skipped when size and options are unchanged. The first callback runs before the
   first paint, and the CSS border-radius fallback (same radii, circular) covers anything not measured yet, so nothing flashes. */
import { insetPath, roundedRectPath, type Radii } from './shape';

export type ShapeOpts = { radii: Radii; smoothing: number; stroke?: { width: number; color: string } };

type Target = { node: Element; opts: ShapeOpts; w: number; h: number; key: string; svg?: SVGSVGElement; path?: SVGPathElement };
const targets = new Map<Element, Target>();
let ro: ResizeObserver | undefined;
const SVGNS = 'http://www.w3.org/2000/svg';

export const cssRadii = ([a, b, c, d]: Radii) => `${a}px ${b}px ${c}px ${d}px`;

function apply(t: Target) {
  if (t.w <= 0 || t.h <= 0) return;
  const { radii, smoothing, stroke } = t.opts;
  const key = `${t.w}|${t.h}|${radii}|${smoothing}|${stroke ? stroke.width + stroke.color : ''}`;
  if (key === t.key) return;
  t.key = key;
  const el = t.node as HTMLElement;
  if (t.node instanceof SVGSVGElement) {
    for (const p of t.node.querySelectorAll<SVGPathElement>('path[data-inset]')) p.setAttribute('d', insetPath(t.w, t.h, radii, smoothing, Number(p.dataset.inset)));
    return;
  }
  el.style.clipPath = `path("${roundedRectPath(t.w, t.h, radii, smoothing)}")`;
  el.style.borderRadius = '0';
  el.dataset.shaped = '';
  if (stroke) {
    if (!t.svg) {
      t.svg = document.createElementNS(SVGNS, 'svg');
      t.svg.setAttribute('aria-hidden', 'true');
      t.svg.style.cssText = 'position:absolute;inset:0;width:100%;height:100%;overflow:visible;pointer-events:none';
      t.path = document.createElementNS(SVGNS, 'path');
      t.path.setAttribute('fill', 'none');
      t.svg.appendChild(t.path);
      el.appendChild(t.svg);
      if (getComputedStyle(el).position === 'static') el.style.position = 'relative';
    }
    t.path!.setAttribute('stroke-width', String(stroke.width));
    t.path!.style.stroke = stroke.color;
    t.path!.setAttribute('d', insetPath(t.w, t.h, radii, smoothing, stroke.width / 2));
  } else if (t.svg) { t.svg.remove(); t.svg = t.path = undefined; }
}

function observer() {
  return (ro ??= new ResizeObserver((entries) => {
    for (const e of entries) {
      const t = targets.get(e.target);
      if (!t) continue;
      const b = e.borderBoxSize?.[0];
      t.w = b ? b.inlineSize : e.contentRect.width;
      t.h = b ? b.blockSize : e.contentRect.height;
      apply(t);
    }
  }));
}

function make(node: Element, opts: ShapeOpts) {
  const t: Target = { node, opts, w: 0, h: 0, key: '' };
  targets.set(node, t);
  observer().observe(node);
  return {
    update(o: ShapeOpts) { t.opts = o; apply(t); },
    destroy() { ro?.unobserve(node); targets.delete(node); t.svg?.remove(); },
  };
}

export const shape = (node: HTMLElement, opts: ShapeOpts) => make(node, opts);
export const shapeRing = (node: SVGSVGElement, opts: ShapeOpts) => make(node, opts);
