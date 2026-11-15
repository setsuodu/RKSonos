package com.rksonos.client.ui.player

import androidx.compose.foundation.background
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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Repeat
import androidx.compose.material.icons.filled.RepeatOne
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Shuffle
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material.icons.filled.SkipPrevious
import androidx.compose.material.icons.filled.VolumeUp
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.rksonos.client.api.RkSonosApi
import com.rksonos.client.api.StatusResponse
import com.rksonos.client.api.Track
import com.rksonos.client.data.Prefs
import com.rksonos.client.ui.theme.Accent
import com.rksonos.client.ui.theme.Danger
import com.rksonos.client.ui.theme.Muted
import com.rksonos.client.ui.theme.Ok
import com.rksonos.client.ui.theme.Panel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import java.io.File
import kotlin.math.roundToInt

@Composable
fun PlayerScreen(
    baseUrl: String,
    prefs: Prefs,
    onSwitchDevice: () -> Unit,
) {
    val api = remember(baseUrl) { RkSonosApi(baseUrl) }
    val scope = rememberCoroutineScope()

    var status by remember { mutableStateOf(StatusResponse()) }
    var log by remember { mutableStateOf("就绪") }
    var connected by remember { mutableStateOf(false) }
    var showSettings by remember { mutableStateOf(false) }
    var settingsUrl by remember { mutableStateOf(baseUrl) }

    val library = remember { mutableStateListOf<Track>() }
    val queue = remember { mutableStateListOf<String>() }
    var queueIndex by remember { mutableStateOf<Int?>(null) }

    var draggingProgress by remember { mutableStateOf(false) }
    var progressValue by remember { mutableFloatStateOf(0f) }
    var draggingVolume by remember { mutableStateOf(false) }
    var volumeValue by remember { mutableFloatStateOf(50f) }

    fun applyStatus(st: StatusResponse) {
        status = st
        connected = true
        if (!draggingVolume) volumeValue = st.volume.toFloat()
        if (!draggingProgress) {
            val dur = st.duration ?: 0.0
            val pos = st.position ?: 0.0
            progressValue = if (dur > 0.5) pos.toFloat().coerceIn(0f, dur.toFloat()) else 0f
        }
        queueIndex = st.queueIndex
    }

    fun runCmd(label: String, block: suspend () -> StatusResponse) {
        scope.launch {
            try {
                applyStatus(block())
                log = "OK"
            } catch (e: Exception) {
                log = "$label 失败: ${e.message}"
            }
        }
    }

    suspend fun refreshAll() {
        try {
            val h = api.health()
            if (!h.equals("ok", true)) throw Exception("health=$h")
            applyStatus(api.getStatus())
            log = "已连接 ${api.baseUrl}"

            val q = api.getQueue()
            queue.clear()
            queue.addAll(q.paths)
            queueIndex = q.index

            val tracks = api.getLibrary(limit = 800)
            library.clear()
            library.addAll(tracks)
        } catch (e: Exception) {
            connected = false
            log = "连接失败: ${e.message}"
        }
    }

    LaunchedEffect(baseUrl) {
        refreshAll()
        while (isActive) {
            delay(1500)
            if (!draggingProgress && !draggingVolume) {
                try {
                    applyStatus(api.getStatus())
                } catch (_: Exception) {
                }
            }
        }
    }

    DisposableEffect(Unit) {
        onDispose { }
    }

    val title = status.path?.let { File(it).name }?.ifBlank { "—" } ?: "—"
    val playing = status.state.equals("playing", true)
    val dur = (status.duration ?: 0.0).toFloat()
    val mode = status.mode

    Column(
        Modifier
            .fillMaxSize()
            .padding(16.dp)
    ) {
        // 顶栏
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text("RKSonos", fontWeight = FontWeight.SemiBold, fontSize = 18.sp)
            IconButton(onClick = {
                settingsUrl = api.baseUrl
                showSettings = true
            }) {
                Icon(Icons.Default.Settings, contentDescription = "设置", tint = Muted)
            }
        }

        // 正在播放
        Card(
            Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(containerColor = Panel),
            shape = RoundedCornerShape(12.dp)
        ) {
            Column(Modifier.padding(16.dp)) {
                Text("正在播放", color = Muted, fontSize = 12.sp)
                Text(
                    title,
                    fontSize = 18.sp,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis
                )
                Spacer(Modifier.height(12.dp))

                // 进度
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(fmtTime(progressValue.toDouble()), color = Muted, fontSize = 12.sp)
                    Slider(
                        value = progressValue,
                        onValueChange = {
                            draggingProgress = true
                            progressValue = it
                        },
                        onValueChangeFinished = {
                            draggingProgress = false
                            runCmd("跳转") { api.seek(progressValue.toDouble()) }
                        },
                        valueRange = 0f..(if (dur > 0.5f) dur else 1f),
                        enabled = dur > 0.5f,
                        modifier = Modifier.weight(1f).padding(horizontal = 8.dp),
                        colors = SliderDefaults.colors(thumbColor = Accent, activeTrackColor = Accent)
                    )
                    Text(fmtTime(dur.toDouble()), color = Muted, fontSize = 12.sp)
                }

                Spacer(Modifier.height(8.dp))

                // 控制：模式 | 上一首 | 播放 | 下一首 …… 音量
                Row(
                    Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    IconButton(onClick = {
                        val next = when (mode) {
                            "normal" -> "repeat_all"
                            "repeat_all" -> "repeat_one"
                            "repeat_one" -> "shuffle"
                            else -> "normal"
                        }
                        runCmd("模式") { api.setMode(next) }
                    }) {
                        val (icon, alpha) = when (mode) {
                            "repeat_one" -> Icons.Default.RepeatOne to 1f
                            "repeat_all" -> Icons.Default.Repeat to 1f
                            "shuffle" -> Icons.Default.Shuffle to 1f
                            else -> Icons.Default.Repeat to 0.4f
                        }
                        Icon(icon, contentDescription = "模式", tint = Accent.copy(alpha = alpha))
                    }
                    IconButton(onClick = { runCmd("上一首") { api.prev() } }) {
                        Icon(Icons.Default.SkipPrevious, contentDescription = "上一首")
                    }
                    Box(
                        Modifier
                            .size(56.dp)
                            .clip(CircleShape)
                            .background(Accent)
                            .clickable {
                                if (playing) runCmd("暂停") { api.pause() }
                                else runCmd("播放") { api.resume() }
                            },
                        contentAlignment = Alignment.Center
                    ) {
                        Icon(
                            if (playing) Icons.Default.Pause else Icons.Default.PlayArrow,
                            contentDescription = "播放/暂停",
                            tint = Color(0xFF12121A),
                            modifier = Modifier.size(28.dp)
                        )
                    }
                    IconButton(onClick = { runCmd("下一首") { api.next() } }) {
                        Icon(Icons.Default.SkipNext, contentDescription = "下一首")
                    }

                    Spacer(Modifier.weight(1f))
                    Icon(Icons.Default.VolumeUp, contentDescription = null, tint = Muted, modifier = Modifier.size(18.dp))
                    Slider(
                        value = volumeValue,
                        onValueChange = {
                            draggingVolume = true
                            volumeValue = it
                        },
                        onValueChangeFinished = {
                            draggingVolume = false
                            runCmd("音量") { api.setVolume(volumeValue.roundToInt()) }
                        },
                        valueRange = 0f..100f,
                        modifier = Modifier.width(110.dp),
                        colors = SliderDefaults.colors(thumbColor = Accent, activeTrackColor = Accent)
                    )
                    Text("${volumeValue.roundToInt()}", color = Muted, fontSize = 12.sp, modifier = Modifier.width(28.dp))
                }
            }
        }

        Spacer(Modifier.height(12.dp))

        // 曲库 + 队列
        Row(Modifier.weight(1f).fillMaxWidth()) {
            Card(
                Modifier.weight(1.2f).fillMaxSize(),
                colors = CardDefaults.cardColors(containerColor = Panel),
                shape = RoundedCornerShape(12.dp)
            ) {
                Column(Modifier.padding(12.dp).fillMaxSize()) {
                    Text("音乐库  ·  ${library.size} 首", fontWeight = FontWeight.SemiBold)
                    Spacer(Modifier.height(8.dp))
                    LazyColumn(Modifier.fillMaxSize()) {
                        itemsIndexed(library, key = { _, t -> t.path }) { _, t ->
                            Text(
                                t.displayName,
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clickable {
                                        runCmd("播放") {
                                            // 以当前列表为队列，从点中曲开始
                                            val paths = library.map { it.path }
                                            val start = paths.indexOf(t.path).coerceAtLeast(0)
                                            api.setQueue(paths, start)
                                        }
                                    }
                                    .padding(vertical = 8.dp),
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis
                            )
                        }
                    }
                }
            }
            Spacer(Modifier.width(10.dp))
            Card(
                Modifier.weight(1f).fillMaxSize(),
                colors = CardDefaults.cardColors(containerColor = Panel),
                shape = RoundedCornerShape(12.dp)
            ) {
                Column(Modifier.padding(12.dp).fillMaxSize()) {
                    val qi = queueIndex
                    val countText = when {
                        queue.isEmpty() -> "— / 0"
                        qi != null && qi >= 0 -> "第 ${qi + 1} / ${queue.size} 首"
                        else -> "— / ${queue.size}"
                    }
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                        Text("播放列表", fontWeight = FontWeight.SemiBold)
                        Text(countText, color = Muted, fontSize = 12.sp)
                    }
                    Spacer(Modifier.height(8.dp))
                    LazyColumn(Modifier.weight(1f)) {
                        itemsIndexed(queue) { i, path ->
                            val cur = queueIndex == i
                            Text(
                                "${i + 1}. ${File(path).name}",
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .background(if (cur) Accent.copy(alpha = 0.2f) else Color.Transparent)
                                    .clickable {
                                        runCmd("播放") { api.setQueue(queue.toList(), i) }
                                    }
                                    .padding(vertical = 8.dp, horizontal = 4.dp),
                                color = if (cur) Accent else Color.Unspecified,
                                fontWeight = if (cur) FontWeight.SemiBold else FontWeight.Normal,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis
                            )
                        }
                    }
                    TextButton(onClick = { runCmd("清空") { api.queueClear() } }) {
                        Text("清空")
                    }
                }
            }
        }

        Spacer(Modifier.height(8.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier
                    .size(8.dp)
                    .clip(CircleShape)
                    .background(if (connected) Ok else Danger)
            )
            Spacer(Modifier.width(6.dp))
            Text(
                if (connected) "已连接" else "未连接",
                color = Muted,
                fontSize = 12.sp
            )
            Spacer(Modifier.width(12.dp))
            Text(
                "状态: ${status.state}  ·  模式: ${status.mode}",
                color = Muted,
                fontSize = 12.sp,
                modifier = Modifier.weight(1f),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis
            )
            Text(log, color = Muted, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }

    if (showSettings) {
        AlertDialog(
            onDismissRequest = { showSettings = false },
            title = { Text("音响设置") },
            text = {
                Column {
                    Text("服务器地址", color = Muted, fontSize = 12.sp)
                    Spacer(Modifier.height(6.dp))
                    OutlinedTextField(
                        value = settingsUrl,
                        onValueChange = { settingsUrl = it },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth()
                    )
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "客户端 0.3.2" + (status.serverVersion?.let { "  ·  服务器 $it" } ?: ""),
                        color = Muted,
                        fontSize = 12.sp
                    )
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    showSettings = false
                    api.baseUrl = settingsUrl
                    scope.launch {
                        prefs.saveLastServer(api.baseUrl)
                        refreshAll()
                    }
                }) { Text("连接") }
            },
            dismissButton = {
                Row {
                    TextButton(onClick = {
                        showSettings = false
                        onSwitchDevice()
                    }) { Text("切换设备") }
                    TextButton(onClick = { showSettings = false }) { Text("关闭") }
                }
            }
        )
    }
}

private fun fmtTime(sec: Double): String {
    val s = if (sec.isNaN() || sec < 0) 0 else sec.toInt()
    val m = s / 60
    val r = s % 60
    return "%d:%02d".format(m, r)
}
