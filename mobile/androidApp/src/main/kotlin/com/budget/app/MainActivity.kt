package com.budget.app

import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
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
            val darkTheme = isSystemInDarkTheme()
            val colorScheme = when {
                Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
                    val context = LocalContext.current
                    if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
                }
                darkTheme -> darkColorScheme()
                else -> lightColorScheme()
            }
            MaterialTheme(colorScheme = colorScheme) {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = MaterialTheme.colorScheme.background,
                ) {
                    val config = currentConfig
                    if (config != null) {
                        DashboardScreen(
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
