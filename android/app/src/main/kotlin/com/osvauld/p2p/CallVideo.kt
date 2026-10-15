package com.osvauld.p2p

import android.Manifest
import android.graphics.Bitmap
import android.os.HandlerThread
import android.util.Log
import android.util.Size
import android.view.Choreographer
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.core.resolutionselector.AspectRatioStrategy
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleOwner
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import uniffi.p2pcore.VideoEvents
import uniffi.p2pcore.VideoInfo
import java.util.concurrent.Executors

/**
 * Video of the current call: the core's state ([info]), our camera (CameraX, only while the call
 * screen is visible) and the remote picture ([RemoteRenderer]). Frames cross into the core only
 * through [io].
 */
class CallVideo(private val app: P2pApp) : VideoEvents {
    private val _info = MutableStateFlow<VideoInfo?>(null)
    val info: StateFlow<VideoInfo?> = _info
    private val _front = MutableStateFlow(true)
    val front: StateFlow<Boolean> = _front
    /** Shown on the call screen, e.g. when the camera permission was refused. */
    private val _problem = MutableStateFlow<String?>(null)
    val problem: StateFlow<String?> = _problem
    /** The HD chip's choice for this call (null = follow Settings). Never written to Settings. */
    private val _hdCall = MutableStateFlow<Boolean?>(null)
    val hdCall: StateFlow<Boolean?> = _hdCall
    fun hdOn(): Boolean = effectivePreferHd(app.preferHdCapture, _hdCall.value)
    /** The chip's short line; [HdNote.id] changes with every new line so the screen can time it out. */
    data class HdNote(val id: Int, val text: String)
    private val _hdNote = MutableStateFlow<HdNote?>(null)
    val hdNote: StateFlow<HdNote?> = _hdNote
    private var noteId = 0
    fun say(text: String) { _hdNote.value = HdNote(++noteId, text) }
    fun noteShown(id: Int) { if (_hdNote.value?.id == id) _hdNote.value = null }
    /** Applies at once: the call screen rebinds the camera when [hdCall] changes. */
    fun setHd(on: Boolean, paused: Boolean) { _hdCall.value = on; say(hdLine(on, paused && on)) }
    val io = VideoIo { app.node }
    /** The camera was on when the call screen went away; it comes back on with the screen. */
    @Volatile private var paused = false
    /** The call was placed with the conversation's video button: the camera turns on once video is negotiated. */
    @Volatile var wantCamera = false
    private val rates = VideoRateMeter()
    @Volatile private var capturePreference = "-"
    @Volatile private var actualCapture = "-"

    override fun onVideoState(info: VideoInfo) {
        testLog("video_state id=${info.callId} negotiated=${info.negotiated} local=${info.localCamera} remote=${info.remoteCamera}")
        val cur = app.calls.ui.value
        if (cur != null && cur.info.callId != info.callId) return
        _info.value = info
        if (wantCamera && info.negotiated && !info.localCamera) { wantCamera = false; setCamera(true) }
    }

    fun setCamera(on: Boolean) {
        paused = false
        if (on && !Perms.granted(app, Manifest.permission.CAMERA)) { _problem.value = "Camera permission needed - they can still hear you"; return }
        _problem.value = null
        try { app.node.setCamera(on) } catch (e: Exception) { Log.w(TAG, "setCamera($on): $e") }
    }

    fun cameraDenied() { _problem.value = "Camera permission needed - they can still hear you" }

    fun flip() { _front.value = !_front.value }

    /** Call screen stopped (backgrounded, minimised, screen off): no capture in the background. */
    fun screenHidden() {
        if (_info.value?.localCamera == true) { setCamera(false); paused = true }
    }

    fun screenShown() {
        if (paused) setCamera(true)
    }

    /** The call ended. */
    fun reset() {
        _info.value = null; _problem.value = null; paused = false; _front.value = true; _hdCall.value = null; _hdNote.value = null
        rates.reset(); capturePreference = "-"; actualCapture = "-"
    }

    fun statsLine(r: RemoteRenderer?): String {
        val i = app.node.videoInfo()
        val s = app.node.videoStats()
        val measured = if (i != null && s != null) rates.sample(System.nanoTime(), VideoRateSnapshot(
            i.callId, s.framesSent, s.framesReceived, s.framesDecoded,
            s.admittedPayloadBytes, s.receivedPayloadBytes,
        )) else { rates.reset(); null }
        fun value(v: Double?) = v?.let { "%.1f".format(java.util.Locale.ROOT, it) } ?: "unknown"
        val rateLine = "target_kbps=${s?.bitrateBps?.div(1000u)} " +
            "admitted_payload_kbps=${value(measured?.admittedPayloadKbps)} received_payload_kbps=${value(measured?.receivedPayloadKbps)} " +
            "admitted_fps=${value(measured?.admittedFps)} received_fps=${value(measured?.receivedFps)} decoded_fps=${value(measured?.decodedFps)} " +
            "capture_pref=$capturePreference capture_actual=$actualCapture "
        return "video_stats negotiated=${i?.negotiated} local=${i?.localCamera} remote=${i?.remoteCamera} " +
            "sent=${s?.framesSent} keys=${s?.keyframesSent} send=${s?.sendWidth}x${s?.sendHeight} kbps=${s?.bitrateBps?.div(1000u)} " +
            rateLine + "admitted_payload_bytes=${s?.admittedPayloadBytes} received_payload_bytes=${s?.receivedPayloadBytes} received=${s?.framesReceived} " +
            "enc_ms=${s?.encodeMs?.let { "%.1f".format(it) }} decoded=${s?.framesDecoded} errors=${s?.decodeErrors} " +
            "recv=${s?.recvWidth}x${s?.recvHeight} path=${if (io.jni) "jni" else "uniffi"} camera=${io.inSize} pushed=${io.pushed} pack_ms=${"%.2f".format(io.packNs / 1e6)} " +
            "push_ms=${"%.2f".format(io.pushNs / 1e6)} push_cpu_ms=${"%.2f".format(io.pushCpuNs / 1e6)} " +
            "rendered=${r?.frames ?: 0} pull_ms=${"%.2f".format(io.pullNs / 1e6)} convert_ms=${"%.2f".format(io.convertNs / 1e6)} render_cpu_ms=${"%.2f".format(io.renderCpuNs / 1e6)}"
    }

    // ---- capture

    private val analysisExec = Executors.newSingleThreadExecutor { Thread(it, "tl-camera") }
    private var bound: ProcessCameraProvider? = null
    @Volatile private var cameraGen = 0

    /** Binds preview + analysis to [owner] (the call screen): CameraX stops them when it stops. */
    fun startCamera(owner: LifecycleOwner, preview: Preview.SurfaceProvider, front: Boolean) {
        val gen = ++cameraGen
        val future = ProcessCameraProvider.getInstance(app)
        future.addListener({
            if (gen != cameraGen) return@addListener  // stopped before the provider was ready
            val provider = try { future.get() } catch (e: Exception) { Log.w(TAG, "camera provider: $e"); return@addListener }
            var cam = if (front) CameraSelector.DEFAULT_FRONT_CAMERA else CameraSelector.DEFAULT_BACK_CAMERA
            if (!provider.hasCamera(cam)) cam = if (front) CameraSelector.DEFAULT_BACK_CAMERA else CameraSelector.DEFAULT_FRONT_CAMERA
            try {
                actualCapture = "-"
                val chosen = bindCapture(capturePreferences(hdOn())) { requested ->
                    val selector = ResolutionSelector.Builder()
                        .setAspectRatioStrategy(if (requested.wide) AspectRatioStrategy.RATIO_16_9_FALLBACK_AUTO_STRATEGY
                            else AspectRatioStrategy.RATIO_4_3_FALLBACK_AUTO_STRATEGY)
                        .setResolutionStrategy(ResolutionStrategy(Size(requested.width, requested.height),
                            ResolutionStrategy.FALLBACK_RULE_CLOSEST_LOWER_THEN_HIGHER))
                        .build()
                    val analysis = ImageAnalysis.Builder()
                        .setResolutionSelector(selector)
                        .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                        .setOutputImageFormat(ImageAnalysis.OUTPUT_IMAGE_FORMAT_YUV_420_888)
                        .build()
                    analysis.setAnalyzer(analysisExec) { img -> img.use {
                        if (gen == cameraGen) {
                            actualCapture = "${it.width}x${it.height}@${it.imageInfo.rotationDegrees}"
                            io.push(it)
                        }
                    } }
                    val pv = Preview.Builder().setResolutionSelector(selector).build().also { it.setSurfaceProvider(preview) }
                    testLog("camera attempt preference=${requested.width}x${requested.height} front=$front")
                    provider.unbindAll()
                    provider.bindToLifecycle(owner, cam, pv, analysis)
                    bound = provider
                }
                capturePreference = "${chosen.width}x${chosen.height}"
                _problem.value = null
                testLog("camera started front=$front preference=$capturePreference (actual size from analysis frames)")
            } catch (e: Exception) {
                Log.w(TAG, "bind camera: $e")
                _problem.value = "Camera unavailable"
            }
        }, ContextCompat.getMainExecutor(app))
    }

    /** Main thread, like [startCamera]. */
    fun stopCamera() {
        cameraGen++
        bound?.unbindAll(); bound = null
        capturePreference = "-"; actualCapture = "-"
        testLog("camera stopped")
    }

    companion object { const val TAG = "CallVideo" }
}

/**
 * Pulls the peer's frames on the display clock (a Choreographer on its own thread), converts them
 * off the main thread and publishes the bitmap; latest wins, so a slow frame is just skipped.
 */
class RemoteRenderer(private val io: VideoIo, private val onFrame: (Bitmap) -> Unit) {
    private val thread = HandlerThread("tl-video-render").apply { start() }
    @Volatile private var running = true
    @Volatile var frames = 0L; private set
    private lateinit var choreographer: Choreographer
    private val tick = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (!running) return
            val b = try { io.pull() } catch (e: Exception) { Log.w(CallVideo.TAG, "pull: $e"); null }
            if (b != null) { frames++; onFrame(b) }
            choreographer.postFrameCallback(this)
        }
    }

    init {
        android.os.Handler(thread.looper).post {
            choreographer = Choreographer.getInstance()
            choreographer.postFrameCallback(tick)
        }
    }

    fun stop() {
        running = false
        thread.quitSafely()
    }
}
