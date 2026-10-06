package com.osvauld.p2p

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

private val Light = lightColorScheme(
    primary = Color(0xFF2F5BEA), onPrimary = Color.White,
    primaryContainer = Color(0xFFDDE3FF), onPrimaryContainer = Color(0xFF001552),
    secondary = Color(0xFF5A5F7A), tertiary = Color(0xFF1E8E5A),
    background = Color(0xFFFBFBFF), surface = Color(0xFFFBFBFF),
    surfaceVariant = Color(0xFFE3E4F0), error = Color(0xFFBA1A1A),
)
private val Dark = darkColorScheme(
    primary = Color(0xFFB6C4FF), onPrimary = Color(0xFF002584),
    primaryContainer = Color(0xFF1741C2), onPrimaryContainer = Color(0xFFDDE3FF),
    secondary = Color(0xFFC2C5E0), tertiary = Color(0xFF6FDBA0),
    background = Color(0xFF121318), surface = Color(0xFF121318),
    surfaceVariant = Color(0xFF45464F), error = Color(0xFFFFB4AB),
)

@Composable
fun P2pTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val ctx = LocalContext.current
    val scheme = when {
        Build.VERSION.SDK_INT >= 31 -> if (dark) dynamicDarkColorScheme(ctx) else dynamicLightColorScheme(ctx)
        dark -> Dark
        else -> Light
    }
    MaterialTheme(colorScheme = scheme, content = content)
}
