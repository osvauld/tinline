package com.osvauld.p2p

import android.graphics.Bitmap
import android.graphics.Color
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel

fun qrBitmap(text: String, size: Int = 640, fg: Int = Color.BLACK, bg: Int = Color.WHITE): Bitmap {
    // M leaves room for the logo in the middle of the code.
    val hints = mapOf(EncodeHintType.ERROR_CORRECTION to ErrorCorrectionLevel.M, EncodeHintType.MARGIN to 1)
    val m = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, size, size, hints)
    val px = IntArray(size * size) { if (m[it % size, it / size]) fg else bg }
    return Bitmap.createBitmap(px, size, size, Bitmap.Config.ARGB_8888)
}
