package com.rksonos.client

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import com.rksonos.client.data.Discovery
import com.rksonos.client.data.Prefs
import com.rksonos.client.ui.discovery.DiscoveryScreen
import com.rksonos.client.ui.player.PlayerScreen
import com.rksonos.client.ui.theme.Bg
import com.rksonos.client.ui.theme.RkSonosTheme
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val prefs = Prefs(applicationContext)

        setContent {
            RkSonosTheme {
                var serverUrl by remember { mutableStateOf<String?>(null) }
                var bootstrapped by remember { mutableStateOf(false) }
                val scope = rememberCoroutineScope()

                // 启动：若上次地址仍可达，直接进播放页
                androidx.compose.runtime.LaunchedEffect(Unit) {
                    val last = prefs.loadLastServer()
                    if (!last.isNullOrBlank()) {
                        val ok = withContext(Dispatchers.IO) { Discovery.tryHealth(last) }
                        if (ok != null) {
                            serverUrl = ok.address
                        }
                    }
                    bootstrapped = true
                }

                androidx.compose.foundation.layout.Box(
                    Modifier
                        .fillMaxSize()
                        .background(Bg)
                        .safeDrawingPadding()
                ) {
                    if (!bootstrapped) {
                        // 短暂空白，避免闪发现页
                    } else if (serverUrl == null) {
                        DiscoveryScreen(
                            prefs = prefs,
                            onConnected = { url ->
                                scope.launch { prefs.saveLastServer(url) }
                                serverUrl = url
                            }
                        )
                    } else {
                        PlayerScreen(
                            baseUrl = serverUrl!!,
                            prefs = prefs,
                            onSwitchDevice = { serverUrl = null }
                        )
                    }
                }
            }
        }
    }
}
