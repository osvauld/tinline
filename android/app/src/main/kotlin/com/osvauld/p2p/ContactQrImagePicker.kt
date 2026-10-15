package com.osvauld.p2p

import android.content.ContentResolver
import android.graphics.ImageDecoder
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.*
import androidx.compose.ui.platform.LocalContext
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

internal class ContactQrImagePicker(val reading: Boolean, val launch: () -> Unit)

/** System photo picker: one image, temporary URI access, no broad gallery/storage permission. */
@Composable
internal fun rememberContactQrImagePicker(onResult: (ContactQrImageResult) -> Unit): ContactQrImagePicker {
    val resolver = LocalContext.current.contentResolver
    val scope = rememberCoroutineScope()
    val result by rememberUpdatedState(onResult)
    var reading by remember { mutableStateOf(false) }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri == null) reading = false // Cancellation is quiet; do not replace the current preview.
        else scope.launch {
            val decoded = withContext(Dispatchers.IO) { readContactQrImage(resolver, uri) }
            reading = false
            result(decoded)
        }
    }
    return ContactQrImagePicker(reading) {
        if (!reading) {
            reading = true
            try { picker.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) }
            catch (_: Exception) { reading = false; result(ContactQrImageResult.Unreadable) }
        }
    }
}

private fun readContactQrImage(resolver: ContentResolver, uri: Uri): ContactQrImageResult = try {
    val bitmap = ImageDecoder.decodeBitmap(ImageDecoder.createSource(resolver, uri)) { decoder, info, _ ->
        val (width, height) = qrImageDimensions(info.size.width, info.size.height)
        decoder.setTargetSize(width, height)
        decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
        decoder.setOnPartialImageListener { false }
    }
    try {
        val pixels = IntArray(bitmap.width * bitmap.height)
        bitmap.getPixels(pixels, 0, bitmap.width, 0, 0, bitmap.width, bitmap.height)
        decodeContactQrPixels(bitmap.width, bitmap.height, pixels)
    } finally { bitmap.recycle() }
} catch (_: Exception) {
    ContactQrImageResult.Unreadable
}
