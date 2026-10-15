package com.osvauld.p2p

import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import org.junit.Assert.*
import org.junit.Test

class ContactQrDecoderTest {
    // Synthetic payloads only. These look like tickets but are deliberately not signed contact invitations.
    private val code = "OSVC2:" + "ABCD2345".repeat(31)
    private val white = -1
    private val black = 0xff000000.toInt()

    private fun qr(text: String, size: Int = 320): IntArray {
        val matrix = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, size, size, mapOf(EncodeHintType.MARGIN to 4))
        return IntArray(size * size) { if (matrix[it % size, it / size]) black else white }
    }
    private fun place(canvas: IntArray, width: Int, pixels: IntArray, side: Int, x: Int, y: Int) {
        repeat(side) { row -> pixels.copyInto(canvas, (y + row) * width + x, row * side, (row + 1) * side) }
    }

    @Test fun readsSyntheticTicketWithoutTreatingItAsValidated() {
        assertEquals(ContactQrImageResult.Decoded(code), decodeContactQrPixels(320, 320, qr(code)))
    }
    @Test fun readsDarkBackgroundQr() {
        val pixels = qr(code).map { if (it == white) black else white }.toIntArray()
        assertEquals(ContactQrImageResult.Decoded(code), decodeContactQrPixels(320, 320, pixels))
    }
    @Test fun readsRotatedScreenshotQr() {
        val original = qr(code)
        val rotated = IntArray(original.size) { i -> original[(319 - i % 320) * 320 + i / 320] }
        assertEquals(ContactQrImageResult.Decoded(code), decodeContactQrPixels(320, 320, rotated))
    }
    @Test fun readsQrInsideLargerImage() {
        val pixels = IntArray(900 * 1400) { white }
        place(pixels, 900, qr(code), 320, 290, 650)
        assertEquals(ContactQrImageResult.Decoded(code), decodeContactQrPixels(900, 1400, pixels))
    }
    @Test fun asksUserToChooseBetweenDifferentQrs() {
        val pixels = IntArray(900 * 480) { white }
        place(pixels, 900, qr("synthetic-one"), 320, 40, 80)
        place(pixels, 900, qr("synthetic-two"), 320, 540, 80)
        assertEquals(ContactQrImageResult.Multiple, decodeContactQrPixels(900, 480, pixels))
    }
    @Test fun repeatedCopiesOfSameQrAreNotAmbiguous() {
        val pixels = IntArray(900 * 480) { white }
        place(pixels, 900, qr(code), 320, 40, 80)
        place(pixels, 900, qr(code), 320, 540, 80)
        assertEquals(ContactQrImageResult.Decoded(code), decodeContactQrPixels(900, 480, pixels))
    }
    @Test fun blankImageHasNoQr() {
        assertEquals(ContactQrImageResult.NoQr, decodeContactQrPixels(400, 400, IntArray(400 * 400) { white }))
    }
    @Test fun arbitraryQrTextIsUntrustedDataNotAnAction() {
        val url = "https://example.invalid/not-a-contact"
        assertEquals(ContactQrImageResult.Decoded(url), decodeContactQrPixels(320, 320, qr(url)))
    }
    @Test fun invalidPixelBuffersAreRejected() {
        assertEquals(ContactQrImageResult.Unreadable, decodeContactQrPixels(320, 320, IntArray(1)))
        assertEquals(ContactQrImageResult.Unreadable, decodeContactQrPixels(0, 0, IntArray(0)))
        assertEquals(ContactQrImageResult.Unreadable, decodeContactQrPixels(Int.MAX_VALUE, Int.MAX_VALUE, IntArray(1)))
    }
    @Test fun boundsMemoryWithoutUpscalingOrLosingAspectRatio() {
        assertEquals(2048 to 1024, qrImageDimensions(8000, 4000))
        assertEquals(1024 to 2048, qrImageDimensions(4000, 8000))
        assertEquals(320 to 320, qrImageDimensions(320, 320))
        assertEquals(1 to 2048, qrImageDimensions(1, 200000))
    }
}
