package com.osvauld.p2p

import android.Manifest
import android.graphics.Bitmap
import android.graphics.Color as AColor
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.rounded.NoPhotography
import androidx.compose.ui.graphics.Color as ComposeColor
import androidx.compose.ui.unit.dp

fun qrBitmap(text: String, size: Int = 640, fg: Int = AColor.BLACK, bg: Int = AColor.WHITE): Bitmap {
    // M leaves room for the logo in the middle of the code.
    val hints = mapOf(EncodeHintType.ERROR_CORRECTION to ErrorCorrectionLevel.M, EncodeHintType.MARGIN to 1)
    val m = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, size, size, hints)
    val px = IntArray(size * size) { if (m[it % size, it / size]) fg else bg }
    return Bitmap.createBitmap(px, size, size, Bitmap.Config.ARGB_8888)
}

/**
 * The camera QR reader used by Add contact and Link a device: asks for the camera, shows the viewfinder
 * with the scan frame and [caption], and hands the first QR text read to [onScanned] (once). [torch]
 * switches the torch; [onGranted] reports whether the camera is allowed (to hide the torch button).
 */
@androidx.compose.runtime.Composable
fun QrCameraBox(modifier: androidx.compose.ui.Modifier, caption: String, torch: Boolean, onGranted: (Boolean) -> Unit, onScanned: (String) -> Unit) {
    val ctx = androidx.compose.ui.platform.LocalContext.current
    val owner = androidx.compose.ui.platform.LocalLifecycleOwner.current
    var granted by androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf(Perms.granted(ctx, Manifest.permission.CAMERA)) }
    val ask = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.RequestPermission()) { granted = it }
    var view by androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf<com.journeyapps.barcodescanner.BarcodeView?>(null) }
    var handled by androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf(false) }
    androidx.compose.runtime.LaunchedEffect(Unit) { if (!granted) ask.launch(Manifest.permission.CAMERA) }
    androidx.compose.runtime.LaunchedEffect(granted) { onGranted(granted) }
    androidx.compose.runtime.LaunchedEffect(torch, view) { view?.setTorch(torch) }
    androidx.compose.runtime.DisposableEffect(owner, view, granted) {
        val v = view
        val o = androidx.lifecycle.LifecycleEventObserver { _, e ->
            if (v != null && granted) when (e) {
                androidx.lifecycle.Lifecycle.Event.ON_RESUME -> v.resume()
                androidx.lifecycle.Lifecycle.Event.ON_PAUSE -> v.pause()
                else -> {}
            }
        }
        owner.lifecycle.addObserver(o)
        if (v != null && granted && owner.lifecycle.currentState.isAtLeast(androidx.lifecycle.Lifecycle.State.RESUMED)) v.resume()
        onDispose { owner.lifecycle.removeObserver(o); v?.pause() }
    }
    val c = Tin.c
    androidx.compose.foundation.layout.Box(modifier.background(ComposeColor(0xFF2A302E)), contentAlignment = androidx.compose.ui.Alignment.Center) {
        if (granted) {
            androidx.compose.ui.viewinterop.AndroidView({ context ->
                com.journeyapps.barcodescanner.BarcodeView(context).apply {
                    decoderFactory = com.journeyapps.barcodescanner.DefaultDecoderFactory(listOf(BarcodeFormat.QR_CODE))
                    decodeSingle(com.journeyapps.barcodescanner.BarcodeCallback { r -> r.text?.let { if (!handled) { handled = true; onScanned(it) } } })
                    view = this
                }
            }, androidx.compose.ui.Modifier.fillMaxSize())
            androidx.compose.foundation.Canvas(androidx.compose.ui.Modifier.size(260.dp)) {
                val len = 44.dp.toPx(); val w = 4.dp.toPx(); val r = 16.dp.toPx(); val s = size.width
                fun corner(x: Float, y: Float, dx: Float, dy: Float) {
                    val p = androidx.compose.ui.graphics.Path().apply {
                        moveTo(x, y + dy * len); lineTo(x, y + dy * r); quadraticTo(x, y, x + dx * r, y); lineTo(x + dx * len, y)
                    }
                    drawPath(p, ComposeColor.White, style = androidx.compose.ui.graphics.drawscope.Stroke(w, cap = androidx.compose.ui.graphics.StrokeCap.Round))
                }
                corner(0f, 0f, 1f, 1f); corner(s, 0f, -1f, 1f); corner(0f, s, 1f, -1f); corner(s, s, -1f, -1f)
                drawRoundRect(ComposeColor(0xFFE8B04A), androidx.compose.ui.geometry.Offset(24.dp.toPx(), s / 2), androidx.compose.ui.geometry.Size(s - 48.dp.toPx(), 2.dp.toPx()), androidx.compose.ui.geometry.CornerRadius(1.dp.toPx()))
            }
            androidx.compose.material3.Text(caption, androidx.compose.ui.Modifier.align(androidx.compose.ui.Alignment.BottomCenter).padding(bottom = 24.dp),
                style = TinType.bodyL.copy(fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold), color = ComposeColor.White)
        } else androidx.compose.foundation.layout.Column(androidx.compose.ui.Modifier.padding(32.dp), horizontalAlignment = androidx.compose.ui.Alignment.CenterHorizontally, verticalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(12.dp)) {
            androidx.compose.material3.Icon(androidx.compose.material.icons.Icons.Rounded.NoPhotography, null, tint = ComposeColor.White, modifier = androidx.compose.ui.Modifier.size(48.dp))
            androidx.compose.material3.Text("Tinline needs the camera to read the code.", style = TinType.bodyL, color = ComposeColor.White, textAlign = androidx.compose.ui.text.style.TextAlign.Center)
            TinButton("Allow camera", { ask.launch(Manifest.permission.CAMERA) }, fill = false)
        }
    }
}
