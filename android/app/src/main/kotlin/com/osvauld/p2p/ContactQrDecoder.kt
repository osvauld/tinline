package com.osvauld.p2p

import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.NotFoundException
import com.google.zxing.RGBLuminanceSource
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.multi.qrcode.QRCodeMultiReader

/** Decoding is not validation. Only core peekCode may turn this untrusted text into a contact preview. */
internal sealed interface ContactQrImageResult {
    data class Decoded(val text: String) : ContactQrImageResult
    data object NoQr : ContactQrImageResult
    data object Multiple : ContactQrImageResult
    data object Unreadable : ContactQrImageResult
}

/** Software-decoded still images are bounded before allocating their pixel arrays. */
internal fun qrImageDimensions(width: Int, height: Int, maxSide: Int = 2048): Pair<Int, Int> {
    require(width > 0 && height > 0 && maxSide > 0)
    val scale = minOf(1.0, maxSide.toDouble() / maxOf(width, height))
    return maxOf(1, (width * scale).toInt()) to maxOf(1, (height * scale).toInt())
}

/** QR only; no URL dispatch, network, contact mutation or logging of decoded payloads. */
internal fun decodeContactQrPixels(width: Int, height: Int, pixels: IntArray): ContactQrImageResult {
    if (width <= 0 || height <= 0 || width > 2048 || height > 2048 || width.toLong() * height != pixels.size.toLong()) {
        return ContactQrImageResult.Unreadable
    }
    val source = RGBLuminanceSource(width, height, pixels)
    val texts = linkedSetOf<String>()
    for (luminance in listOf(source, source.invert())) {
        try {
            val results = QRCodeMultiReader().decodeMultiple(
                BinaryBitmap(HybridBinarizer(luminance)), mapOf(DecodeHintType.TRY_HARDER to true),
            )
            results.mapNotNullTo(texts) { it.text?.takeIf(String::isNotBlank) }
        } catch (_: NotFoundException) {
            // No QR in this polarity. Dark-background screenshots may need the inverted pass.
        }
    }
    return when (texts.size) {
        0 -> ContactQrImageResult.NoQr
        1 -> ContactQrImageResult.Decoded(texts.single())
        else -> ContactQrImageResult.Multiple
    }
}
