package com.osvauld.p2p

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

@Composable
fun Dot(color: Color, size: Dp = 8.dp, hollow: Boolean = false) {
    Canvas(Modifier.size(size)) {
        if (hollow) drawCircle(color, radius = this.size.width / 2 - 1.dp.toPx(), style = Stroke(2.dp.toPx()))
        else drawCircle(color)
    }
}
