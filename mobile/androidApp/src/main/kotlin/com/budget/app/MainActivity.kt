package com.budget.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import com.budget.shared.config.AndroidConfigStore
import com.budget.shared.config.ServerConfig

class MainActivity : ComponentActivity() {

    private lateinit var configStore: AndroidConfigStore
    private var currentConfig by mutableStateOf<ServerConfig?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        configStore = AndroidConfigStore(applicationContext)
        currentConfig = configStore.load()

        setContent {
            MaterialTheme {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = MaterialTheme.colorScheme.background,
                ) {
                    val config = currentConfig
                    if (config != null) {
                        MainScreen(
                            config = config,
                            onLogout = {
                                configStore.clear()
                                currentConfig = null
                            },
                        )
                    } else {
                        SetupScreen(
                            configStore = configStore,
                            onConnected = { newConfig ->
                                currentConfig = newConfig
                            },
                        )
                    }
                }
            }
        }
    }
}
