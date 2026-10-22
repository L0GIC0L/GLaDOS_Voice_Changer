package com.l0gic0l.glados

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.l0gic0l.glados.ui.MainScreen
import com.l0gic0l.glados.ui.theme.GladosTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        setContent {
            val vm: MainViewModel = viewModel()
            val theme by vm.theme.collectAsStateWithLifecycle()
            GladosTheme(theme) { MainScreen(vm) }
        }
    }
}
