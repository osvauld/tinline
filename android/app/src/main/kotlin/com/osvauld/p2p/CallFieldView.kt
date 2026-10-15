package com.osvauld.p2p

import android.graphics.RuntimeShader
import android.os.Build
import android.os.SystemClock
import androidx.annotation.RequiresApi
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.ShaderBrush
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.layout
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.IntSize
import kotlinx.coroutines.delay
import kotlin.math.exp
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt
import kotlin.math.sqrt

/*
 * The call screen's moving colour field (Fog), drawn behind the voice call screens. The model is CallFieldModel.kt
 * (parity with the playground and desktop). Drawing mirrors crates/desktop/src/call_field.wgsl / gl.ts:
 *  - Android 13+ (API 33): an AGSL RuntimeShader, the same Fog maths as the WGSL shader.
 *  - Older: the five lobes as radial gradients (gl.ts cssRenderer), plus the dark-mode calm patches.
 * Resolution is capped like gl.ts (device pixel ratio 1.5, 700k pixels) by drawing into a smaller offscreen layer that is
 * scaled up. The loop sleeps when nothing animates (end/failed fade finished, reduced motion between notes).
 */

/** Dark mode: calm patches behind text (0 = none) and the chroma gain (CloudField.svelte SCRIM_DARK / GAIN_DARK). */
private const val SCRIM_DARK = 0.45f
private const val GAIN_DARK = 1.55f

/** A calm patch behind text or controls: centre and half size, in dp, relative to the field. */
internal data class FieldCalm(val cx: Float, val cy: Float, val hw: Float, val hh: Float)

/** gl.ts resize(): canvas px per CSS px is min(dpr, 1.5), and no more than 700k pixels in all. Returns the layer scale (<= 1). */
internal fun fieldRenderScale(wDp: Float, hDp: Float, density: Float): Float {
    var s = min(density, 1.5f)
    if (wDp * hDp * s * s > 700_000f) s = sqrt(700_000f / (wDp * hDp))
    return min(1f, s / density)
}

private class FieldHolder {
    var model: CallField? = null
    var frame: FieldFrame? = null
    val origin = SystemClock.elapsedRealtime()
    fun nowMs(): Double = (SystemClock.elapsedRealtime() - origin).toDouble()
}

/** Fills its parent with the field. Layout points (avatar, calm) are in dp inside this box. */
@Composable
internal fun CallFieldBackground(state: CallFieldState, muted: Boolean, avatarDp: Offset, calm: List<FieldCalm>, modifier: Modifier = Modifier) {
    val dark = Tin.c.dark
    val reduced = reducedMotion()
    val density = LocalDensity.current.density
    var size by remember { mutableStateOf(IntSize.Zero) }
    val holder = remember { FieldHolder() }
    var tick by remember { mutableLongStateOf(0L) }
    val agsl = remember { if (Build.VERSION.SDK_INT >= 33) runCatching { FieldAgsl() }.getOrNull() else null }

    LaunchedEffect(state, dark, reduced, muted, avatarDp, size, density) {
        if (size.width == 0 || size.height == 0) return@LaunchedEffect
        val theme = if (dark) FieldTheme.Dark else FieldTheme.Light
        val m = holder.model ?: CallField(theme, reduced).also { holder.model = it }
        m.setTheme(theme); m.setReduced(reduced); m.setMute(muted)
        m.setView(size.width / density.toDouble(), size.height / density.toDouble(), avatarDp.x.toDouble(), avatarDp.y.toDouble())
        if (m.state != state) { val now = holder.nowMs(); m.setState(state, now, now + state.cueDelayMs(reduced)) }
        while (true) {
            val t = holder.nowMs()
            val fr = m.update(t)
            holder.frame = fr
            tick++
            if (fr.animating) { withFrameNanos { }; continue }
            val wake = fr.wakeAt ?: break
            delay(max(0.0, wake - t).toLong() + 1)
        }
    }

    val wDp = size.width / density
    val hDp = size.height / density
    val scale = if (agsl != null && size.width > 0) fieldRenderScale(wDp, hDp, density) else 1f
    FieldShell(modifier, scale, size, onSize = { size = it }, Modifier.drawBehind {
        if (tick < 0) return@drawBehind // reads the state: redraw whenever the loop produced a frame
        val fr = holder.frame ?: return@drawBehind
        val k = if (dark) SCRIM_DARK else 0f
        val gain = if (dark) GAIN_DARK else 1f
        if (agsl != null) agsl.draw(this, fr, calm, wDp, hDp, k, gain)
        else drawGradients(fr, calm, this.size.width, this.size.height, density, k, gain)
    })
}

/** The layout shell: fills the parent; when [scale] < 1 its content is measured at that fraction, rendered into an offscreen layer and scaled back up. */
@Composable
private fun FieldShell(modifier: Modifier, scale: Float, full: IntSize, onSize: (IntSize) -> Unit, content: Modifier) {
    val low = if (scale < 1f) Modifier.layout { m, c ->
        val w = c.maxWidth; val h = c.maxHeight
        val p = m.measure(Constraints.fixed((w * scale).roundToInt().coerceAtLeast(1), (h * scale).roundToInt().coerceAtLeast(1)))
        layout(w, h) { p.place(0, 0) }
    }.graphicsLayer {
        scaleX = full.width.toFloat() / size.width.coerceAtLeast(1f)
        scaleY = full.height.toFloat() / size.height.coerceAtLeast(1f)
        transformOrigin = TransformOrigin(0f, 0f)
        compositingStrategy = CompositingStrategy.Offscreen
    } else Modifier
    androidx.compose.foundation.layout.Box(modifier.fillMaxSize().onSizeChanged(onSize).clipToBounds().then(low).then(content))
}

// ------------------------------------------------------------------ gradient fallback (API < 33)

private fun rgbColor(c: DoubleArray, a: Double): Color =
    Color((c[0] / 255.0).toFloat().coerceIn(0f, 1f), (c[1] / 255.0).toFloat().coerceIn(0f, 1f), (c[2] / 255.0).toFloat().coerceIn(0f, 1f), a.toFloat().coerceIn(0f, 1f))

/** gl.ts cssRenderer: one radial gradient per lobe over the theme background, calm patches on top. Sizes are in px here. */
private fun DrawScope.drawGradients(f: FieldFrame, calm: List<FieldCalm>, w: Float, h: Float, d: Float, k: Float, gain: Float) {
    val sh = min(w, h)
    val bg = FieldMath.labToRgb(f.bg)
    drawRect(rgbColor(bg, 1.0))
    for (l in f.lobes) {
        val c = FieldMath.labToRgb(doubleArrayOf(l.lab[0], l.lab[1] * gain, l.lab[2] * gain))
        val a = f.cap * (1 - exp(-l.alpha))
        val ctr = Offset(w / 2f + (l.x * sh).toFloat(), h / 2f + (l.y * sh).toFloat())
        val r = (l.sigma * sh * 2.2).toFloat().coerceAtLeast(1f)
        drawRect(Brush.radialGradient(0f to rgbColor(c, a), .45f to rgbColor(c, a * .45), 1f to rgbColor(c, 0.0), center = ctr, radius = r))
    }
    if (k > 0f) for (p in calm.take(2)) {
        val rx = ((p.hw + 44f) / .55f * d).coerceAtLeast(1f); val ry = ((p.hh + 30f) / .55f * d).coerceAtLeast(1f)
        val ctr = Offset(p.cx * d, p.cy * d)
        withTransform({ translate(ctr.x, ctr.y); scale(1f, ry / rx, pivot = Offset.Zero) }) {
            drawCircle(Brush.radialGradient(0f to rgbColor(bg, k.toDouble()), .55f to rgbColor(bg, k.toDouble()), 1f to rgbColor(bg, 0.0), center = Offset.Zero, radius = rx), radius = rx, center = Offset.Zero)
        }
    }
}

// ------------------------------------------------------------------ AGSL (API 33+)

/** The Fog branch of call_field.wgsl in AGSL. Inputs are in the short-side-normalised space of the model. */
private const val FOG_AGSL = """
uniform float2 res;
uniform float tau;
uniform float warp;
uniform float cap;
uniform float ws;
uniform float nr;
uniform float3 bg;
uniform float gain;
uniform float scrimK;
uniform float4 patch[2];
uniform float4 lobe[5];
uniform float3 lobLab[5];

float2 rot(float2 p) { return float2(0.8 * p.x - 0.6 * p.y, 0.6 * p.x + 0.8 * p.y); }
float2 h23(float3 p0) {
    float3 p = fract(p0 * float3(0.1031, 0.1030, 0.0973));
    p += dot(p, p.yzx + 33.33);
    return fract((p.xx + p.yz) * p.zy) * 2.0 - 1.0;
}
float2 vn(float3 p) {
    float3 i = floor(p); float3 f = fract(p);
    float3 q = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    float2 a = mix(mix(h23(i), h23(i + float3(1.0, 0.0, 0.0)), q.x), mix(h23(i + float3(0.0, 1.0, 0.0)), h23(i + float3(1.0, 1.0, 0.0)), q.x), q.y);
    float2 b = mix(mix(h23(i + float3(0.0, 0.0, 1.0)), h23(i + float3(1.0, 0.0, 1.0)), q.x), mix(h23(i + float3(0.0, 1.0, 1.0)), h23(i + float3(1.0, 1.0, 1.0)), q.x), q.y);
    return mix(a, b, q.z);
}
float3 ok2lin(float3 c) {
    float l = c.x + 0.3963377774 * c.y + 0.2158037573 * c.z;
    float m = c.x - 0.1055613458 * c.y - 0.0638541728 * c.z;
    float s = c.x - 0.0894841775 * c.y - 1.2914855480 * c.z;
    l = l * l * l; m = m * m * m; s = s * s * s;
    return float3(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
                  -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
                  -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s);
}
float l2s1(float v) {
    float c = clamp(v, 0.0, 1.0);
    return c < 0.0031308 ? 12.92 * c : 1.055 * pow(c, 1.0 / 2.4) - 0.055;
}

half4 main(float2 fc) {
    float short_ = min(res.x, res.y);
    float2 p = (fc - 0.5 * res) / short_;
    float3 lab = bg;
    float z = tau * nr;
    float2 w = vn(float3(p * ws, z)) * 0.55;
    float2 p2 = rot(p);
    w += vn(float3(p2 * ws * 2.07 + 7.3, z * 1.3 + 3.0)) * 0.30;
    float2 p3 = rot(p2);
    w += vn(float3(p3 * ws * 4.13 + 13.1, z * 1.7 + 9.0)) * 0.15;
    float2 q = p + warp * w;
    // Coverage S sums the lobes, colour is their weighted mean; light restores the mean's chroma, dark mostly doesn't.
    float S = 0.0; float W = 0.0; float3 ab = float3(0.0); float cs = 0.0;
    for (int i = 0; i < 5; i++) {
        float4 l = lobe[i];
        float2 d = q - l.xy;
        float a = l.w * exp(-dot(d, d) / (2.0 * l.z * l.z));
        S += a; W += a + 1e-9; ab += (a + 1e-9) * lobLab[i]; cs += (a + 1e-9) * length(lobLab[i].yz);
    }
    if (S > 1e-5) {
        float3 m = ab / W; cs /= W;
        float cm = length(m.yz);
        float ct = mix(cm, cs, gain > 1.01 ? 0.25 : 1.0);
        if (cm > 1e-4) { m = float3(m.x, m.yz * (ct / cm)); }
        lab = mix(bg, m, cap * (1.0 - exp(-S)));
    }
    // Calm patches behind the text block and the controls (flat inside, soft edge).
    float sc = 0.0;
    for (int i = 0; i < 2; i++) {
        float2 d = (p - patch[i].xy) / patch[i].zw;
        sc = max(sc, 1.0 - smoothstep(0.0, 1.0, length(d)));
    }
    lab = mix(lab, bg, scrimK * sc);
    lab = float3(lab.x, lab.yz * gain);
    float3 lin = ok2lin(lab);
    float3 rgb = float3(l2s1(lin.x), l2s1(lin.y), l2s1(lin.z));
    float2 glc = float2(fc.x, res.y - fc.y);   // the browser's bottom-up gl_FragCoord, for the same dither
    float n = fract(52.9829189 * fract(dot(glc, float2(0.06711056, 0.00583715))));
    rgb += (n - 0.5) / 255.0;
    return half4(half3(rgb), 1.0);
}
"""

@RequiresApi(33)
private class FieldAgsl {
    private val shader = RuntimeShader(FOG_AGSL)
    private val brush = ShaderBrush(shader)
    private val lobe = FloatArray(20)
    private val lab = FloatArray(15)
    private val patch = FloatArray(8)

    /** [calm], [wDp], [hDp] are in dp; the scope's own size is the (possibly reduced) pixel size. */
    fun draw(scope: DrawScope, f: FieldFrame, calm: List<FieldCalm>, wDp: Float, hDp: Float, k: Float, gain: Float) {
        val sh = min(wDp, hDp)
        shader.setFloatUniform("res", scope.size.width, scope.size.height)
        shader.setFloatUniform("tau", f.tau.toFloat())
        shader.setFloatUniform("warp", f.warp.toFloat())
        shader.setFloatUniform("cap", f.cap.toFloat())
        shader.setFloatUniform("ws", FieldTune.WARP_SCALE.toFloat())
        shader.setFloatUniform("nr", FieldTune.NOISE_RATE.toFloat())
        shader.setFloatUniform("bg", f.bg[0].toFloat(), f.bg[1].toFloat(), f.bg[2].toFloat())
        shader.setFloatUniform("gain", gain)
        // Calm patches (CloudField.svelte measureScrim): radii are (half size + 44 / 30) / .55, in short-side units.
        patch.fill(0f)
        for (i in 0 until 2) {
            val c = calm.getOrNull(i)
            if (c == null) { patch[4 * i + 2] = 1f; patch[4 * i + 3] = 1f; continue }
            patch[4 * i] = (c.cx - wDp / 2f) / sh; patch[4 * i + 1] = (c.cy - hDp / 2f) / sh
            patch[4 * i + 2] = (c.hw + 44f) / .55f / sh; patch[4 * i + 3] = (c.hh + 30f) / .55f / sh
        }
        shader.setFloatUniform("patch", patch)
        shader.setFloatUniform("scrimK", if (calm.isEmpty()) 0f else k)
        for ((i, l) in f.lobes.withIndex()) {
            lobe[4 * i] = l.x.toFloat(); lobe[4 * i + 1] = l.y.toFloat(); lobe[4 * i + 2] = l.sigma.toFloat(); lobe[4 * i + 3] = l.alpha.toFloat()
            lab[3 * i] = l.lab[0].toFloat(); lab[3 * i + 1] = l.lab[1].toFloat(); lab[3 * i + 2] = l.lab[2].toFloat()
        }
        shader.setFloatUniform("lobe", lobe)
        shader.setFloatUniform("lobLab", lab)
        scope.drawRect(brush)
    }
}
