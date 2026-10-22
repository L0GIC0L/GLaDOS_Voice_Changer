package com.l0gic0l.glados.ui

import android.os.Build
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.GraphicEq
import androidx.compose.material.icons.filled.Palette
import androidx.compose.material.icons.filled.RecordVoiceOver
import androidx.compose.material.icons.filled.Tune
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CenterAlignedTopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.l0gic0l.glados.MainViewModel
import com.l0gic0l.glados.data.DarkMode
import com.l0gic0l.glados.data.ThemeSettings

enum class Dest(val label: String, val icon: ImageVector) {
    SPEAK("Speak", Icons.Filled.RecordVoiceOver),
    EDITOR("Editor", Icons.Filled.GraphicEq),
    VOICE("Voice", Icons.Filled.Tune),
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MainScreen(vm: MainViewModel) {
    var dest by rememberSaveable { mutableStateOf(Dest.SPEAK) }
    var showTheme by remember { mutableStateOf(false) }
    val snackbar = remember { SnackbarHostState() }
    val holder = rememberSaveableStateHolder()
    val theme by vm.theme.collectAsStateWithLifecycle()

    LaunchedEffect(vm.message) {
        vm.message?.let {
            snackbar.showSnackbar(it)
            vm.message = null
        }
    }

    Scaffold(
        topBar = {
            CenterAlignedTopAppBar(
                title = { Text("GLaDOS Voice") },
                actions = {
                    IconButton(onClick = { showTheme = true }, modifier = Modifier.testTag("theme-button")) { Icon(Icons.Filled.Palette, contentDescription = "Theme") }
                },
            )
        },
        bottomBar = {
            NavigationBar {
                Dest.entries.forEach {
                    NavigationBarItem(
                        selected = dest == it,
                        onClick = { dest = it },
                        icon = { Icon(it.icon, contentDescription = null) },
                        label = { Text(it.label) },
                        modifier = Modifier.testTag("nav-${it.name.lowercase()}"),
                    )
                }
            }
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        Box(Modifier.padding(padding).fillMaxSize()) {
            // Keeps each tab's remembered state (typed text, scroll position, expanded sections) while another tab is shown.
            holder.SaveableStateProvider(dest.name) {
                when (dest) {
                    Dest.SPEAK -> TtsScreen(vm, openEditor = { dest = Dest.EDITOR })
                    Dest.EDITOR -> EditorScreen(vm, goSpeak = { dest = Dest.SPEAK })
                    Dest.VOICE -> VoiceScreen(vm)
                }
            }
            vm.busy?.let { label ->
                Surface(
                    tonalElevation = 6.dp,
                    shadowElevation = 4.dp,
                    modifier = Modifier.align(Alignment.TopCenter).fillMaxWidth(),
                ) {
                    Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text(label, style = MaterialTheme.typography.labelLarge)
                        LinearProgressIndicator(Modifier.fillMaxWidth())
                    }
                }
            }
        }
    }

    if (showTheme) {
        ThemeDialog(theme, onChange = vm::setTheme, onDismiss = { showTheme = false })
    }
}

@Composable
private fun ThemeDialog(theme: ThemeSettings, onChange: (ThemeSettings) -> Unit, onDismiss: () -> Unit) {
    val dynamicAvailable = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Theme") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text("Wallpaper colors", style = MaterialTheme.typography.bodyLarge)
                        Text(
                            if (dynamicAvailable) "Material You colors taken from your wallpaper." else "Needs Android 12 or newer. Using Aperture orange.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    Switch(
                        checked = theme.dynamicColor && dynamicAvailable,
                        onCheckedChange = { onChange(ThemeSettings(it, theme.darkMode)) },
                        enabled = dynamicAvailable,
                    )
                }
                Text("Appearance", style = MaterialTheme.typography.bodyLarge)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for ((mode, label) in listOf(DarkMode.SYSTEM to "System", DarkMode.LIGHT to "Light", DarkMode.DARK to "Dark")) {
                        FilterChip(
                            selected = theme.darkMode == mode,
                            onClick = { onChange(ThemeSettings(theme.dynamicColor, mode)) },
                            label = { Text(label) },
                        )
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Done") } },
    )
}
