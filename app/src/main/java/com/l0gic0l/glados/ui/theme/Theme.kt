package com.l0gic0l.glados.ui.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import com.l0gic0l.glados.data.DarkMode
import com.l0gic0l.glados.data.ThemeSettings

/** Aperture orange scheme, used when wallpaper colors are off or unavailable (before Android 12). */
private val ApertureLight = lightColorScheme(
    primary = Color(0xFF8B5000), onPrimary = Color(0xFFFFFFFF),
    primaryContainer = Color(0xFFFFDCBE), onPrimaryContainer = Color(0xFF2C1600),
    secondary = Color(0xFF725A42), onSecondary = Color(0xFFFFFFFF),
    secondaryContainer = Color(0xFFFDDDBF), onSecondaryContainer = Color(0xFF291806),
    tertiary = Color(0xFF00639A), onTertiary = Color(0xFFFFFFFF),
    tertiaryContainer = Color(0xFFCDE5FF), onTertiaryContainer = Color(0xFF001D32),
    error = Color(0xFFBA1A1A), onError = Color(0xFFFFFFFF),
    errorContainer = Color(0xFFFFDAD6), onErrorContainer = Color(0xFF410002),
    background = Color(0xFFFFF8F5), onBackground = Color(0xFF201A17),
    surface = Color(0xFFFFF8F5), onSurface = Color(0xFF201A17),
    surfaceVariant = Color(0xFFF2DFD1), onSurfaceVariant = Color(0xFF51443A),
    outline = Color(0xFF84746A), outlineVariant = Color(0xFFD6C3B6),
    scrim = Color(0xFF000000),
    inverseSurface = Color(0xFF362F2B), inverseOnSurface = Color(0xFFFBEEE8), inversePrimary = Color(0xFFFFB870),
    surfaceTint = Color(0xFF8B5000),
    surfaceDim = Color(0xFFE2D8D2), surfaceBright = Color(0xFFFFF8F5),
    surfaceContainerLowest = Color(0xFFFFFFFF), surfaceContainerLow = Color(0xFFFCF1EA),
    surfaceContainer = Color(0xFFF6ECE5), surfaceContainerHigh = Color(0xFFF1E6DF),
    surfaceContainerHighest = Color(0xFFEBE0D9),
)

private val ApertureDark = darkColorScheme(
    primary = Color(0xFFFFB870), onPrimary = Color(0xFF4A2800),
    primaryContainer = Color(0xFF6A3B00), onPrimaryContainer = Color(0xFFFFDCBE),
    secondary = Color(0xFFE1C1A3), onSecondary = Color(0xFF402C17),
    secondaryContainer = Color(0xFF58422B), onSecondaryContainer = Color(0xFFFDDDBF),
    tertiary = Color(0xFF97CBFF), onTertiary = Color(0xFF003353),
    tertiaryContainer = Color(0xFF004A77), onTertiaryContainer = Color(0xFFCDE5FF),
    error = Color(0xFFFFB4AB), onError = Color(0xFF690005),
    errorContainer = Color(0xFF93000A), onErrorContainer = Color(0xFFFFDAD6),
    background = Color(0xFF18120E), onBackground = Color(0xFFEDE0D9),
    surface = Color(0xFF18120E), onSurface = Color(0xFFEDE0D9),
    surfaceVariant = Color(0xFF51443A), onSurfaceVariant = Color(0xFFD6C3B6),
    outline = Color(0xFF9F8D81), outlineVariant = Color(0xFF51443A),
    scrim = Color(0xFF000000),
    inverseSurface = Color(0xFFEDE0D9), inverseOnSurface = Color(0xFF362F2B), inversePrimary = Color(0xFF8B5000),
    surfaceTint = Color(0xFFFFB870),
    surfaceDim = Color(0xFF18120E), surfaceBright = Color(0xFF3F3732),
    surfaceContainerLowest = Color(0xFF130D09), surfaceContainerLow = Color(0xFF201A17),
    surfaceContainer = Color(0xFF251E1B), surfaceContainerHigh = Color(0xFF302925),
    surfaceContainerHighest = Color(0xFF3B3430),
)

@Composable
fun GladosTheme(settings: ThemeSettings, content: @Composable () -> Unit) {
    val dark = when (settings.darkMode) {
        DarkMode.SYSTEM -> isSystemInDarkTheme()
        DarkMode.LIGHT -> false
        DarkMode.DARK -> true
    }
    val context = LocalContext.current
    val scheme: ColorScheme = when {
        settings.dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S ->
            if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        dark -> ApertureDark
        else -> ApertureLight
    }
    MaterialTheme(colorScheme = scheme, content = content)
}
