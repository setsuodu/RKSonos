package com.rksonos.client.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

val Bg = Color(0xFF1E1E2E)
val Panel = Color(0xFF2A2A3C)
val Accent = Color(0xFF7C9CFF)
val TextPrimary = Color(0xFFE8E8F0)
val Muted = Color(0xFF9A9AB0)
val Ok = Color(0xFF6BCB77)
val Danger = Color(0xFFFF6B6B)

private val Scheme = darkColorScheme(
    primary = Accent,
    onPrimary = Color(0xFF12121A),
    background = Bg,
    onBackground = TextPrimary,
    surface = Panel,
    onSurface = TextPrimary,
    secondary = Muted,
    onSecondary = TextPrimary,
)

@Composable
fun RkSonosTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = Scheme, content = content)
}
