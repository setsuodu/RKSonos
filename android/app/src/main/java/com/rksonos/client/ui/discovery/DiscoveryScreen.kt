package com.rksonos.client.ui.discovery

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.rksonos.client.api.DiscoveredDevice
import com.rksonos.client.data.Discovery
import com.rksonos.client.data.Prefs
import com.rksonos.client.ui.theme.Accent
import com.rksonos.client.ui.theme.Muted
import com.rksonos.client.ui.theme.Panel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@Composable
fun DiscoveryScreen(
    prefs: Prefs,
    onConnected: (String) -> Unit,
) {
    val devices = remember { mutableStateListOf<DiscoveredDevice>() }
    var scanning by remember { mutableStateOf(false) }
    var scanState by remember { mutableStateOf("正在搜索…") }
    var status by remember { mutableStateOf("") }
    var manual by remember { mutableStateOf("") }
    var selected by remember { mutableStateOf<DiscoveredDevice?>(null) }
    val scope = rememberCoroutineScope()

    fun startScan() {
        scope.launch {
            scanning = true
            scanState = "正在搜索…"
            status = ""
            devices.clear()
            selected = null
            try {
                val last = prefs.loadLastServer()
                val hints = buildList {
                    if (manual.isNotBlank()) add(manual.trim())
                    if (!last.isNullOrBlank()) add(last)
                }
                Discovery.scan(hints) { d ->
                    // 回调可能在 IO 线程
                    scope.launch {
                        if (devices.none { it.host == d.host && it.port == d.port }) {
                            devices.add(d)
                            if (selected == null) selected = d
                            scanState = "找到 ${devices.size} 台"
                        }
                    }
                }
                if (devices.isEmpty()) {
                    scanState = "未找到音箱"
                    status = "请确认音箱已开机，并与手机在同一 Wi‑Fi，或在下方手动输入地址。"
                } else {
                    scanState = "找到 ${devices.size} 台"
                }
            } catch (_: Exception) {
                scanState = "搜索失败"
                status = "请稍后点「刷新」重试，或手动输入地址。"
            } finally {
                scanning = false
            }
        }
    }

    fun connect(url: String) {
        scope.launch {
            status = "正在连接…"
            val ok = withContext(Dispatchers.IO) { Discovery.tryHealth(url) }
            if (ok == null) {
                status = "连接失败，请确认音箱已开机且地址正确。"
                return@launch
            }
            prefs.saveLastServer(ok.address)
            onConnected(ok.address)
        }
    }

    LaunchedEffect(Unit) { startScan() }

    Column(
        Modifier
            .fillMaxSize()
            .padding(24.dp)
    ) {
        Text("选择音箱", fontSize = 22.sp, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(16.dp))

        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(scanState, color = Muted)
            TextButton(onClick = { if (!scanning) startScan() }, enabled = !scanning) {
                Text("刷新")
            }
        }

        Spacer(Modifier.height(8.dp))

        Box(Modifier.weight(1f).fillMaxWidth()) {
            if (scanning && devices.isEmpty()) {
                CircularProgressIndicator(
                    Modifier.align(Alignment.Center),
                    color = Accent
                )
            }
            LazyColumn(Modifier.fillMaxSize()) {
                items(devices, key = { "${it.host}:${it.port}" }) { d ->
                    val isSel = selected?.host == d.host && selected?.port == d.port
                    Card(
                        Modifier
                            .fillMaxWidth()
                            .padding(vertical = 4.dp)
                            .clickable { selected = d },
                        colors = CardDefaults.cardColors(
                            containerColor = if (isSel) Accent.copy(alpha = 0.25f) else Panel
                        ),
                        shape = RoundedCornerShape(10.dp)
                    ) {
                        Column(Modifier.padding(horizontal = 14.dp, vertical = 12.dp)) {
                            Text(d.display, fontWeight = FontWeight.SemiBold, fontSize = 15.sp)
                            Text(d.address, color = Muted, fontSize = 12.sp)
                        }
                    }
                }
            }
        }

        Spacer(Modifier.height(12.dp))
        Button(
            onClick = {
                val d = selected
                if (d == null) status = "请先点选一台音箱。"
                else connect(d.address)
            },
            Modifier
                .fillMaxWidth()
                .height(48.dp),
            shape = RoundedCornerShape(10.dp)
        ) {
            Text("连接", fontSize = 15.sp)
        }

        Spacer(Modifier.height(18.dp))
        Text("没有找到？在下方输入", color = Muted, fontSize = 12.sp)
        Spacer(Modifier.height(6.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                value = manual,
                onValueChange = { manual = it },
                modifier = Modifier.weight(1f),
                singleLine = true,
                placeholder = {
                    Text(
                        "例如 http://192.168.1.252:8080",
                        fontStyle = FontStyle.Italic,
                        color = Muted.copy(alpha = 0.7f)
                    )
                },
                colors = OutlinedTextFieldDefaults.colors(
                    focusedBorderColor = Accent,
                    unfocusedBorderColor = Muted.copy(alpha = 0.4f),
                )
            )
            Spacer(Modifier.padding(4.dp))
            Button(
                onClick = {
                    if (manual.isBlank()) status = "请输入地址，例如 http://192.168.1.252:8080"
                    else connect(manual.trim())
                },
                shape = RoundedCornerShape(10.dp)
            ) { Text("连接") }
        }

        if (status.isNotBlank()) {
            Spacer(Modifier.height(12.dp))
            Text(status, color = Muted, fontSize = 12.sp)
        }
    }
}
