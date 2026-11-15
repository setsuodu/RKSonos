package com.rksonos.client.ui.player

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.aspectRatio
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
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.material.icons.filled.LibraryMusic
import androidx.compose.material.icons.filled.QueueMusic
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
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
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.rksonos.client.api.BrowseItem
import com.rksonos.client.api.BrowseKind
import com.rksonos.client.api.RkSonosApi
import com.rksonos.client.api.StatusResponse
import com.rksonos.client.data.LibraryBrowser
import com.rksonos.client.data.Prefs
import com.rksonos.client.ui.theme.Accent
import com.rksonos.client.ui.theme.Danger
import com.rksonos.client.ui.theme.Muted
import com.rksonos.client.ui.theme.Ok
import com.rksonos.client.ui.theme.Panel
import com.rksonos.client.ui.theme.TextPrimary
import coil.compose.AsyncImage
import coil.compose.SubcomposeAsyncImage
import coil.request.ImageRequest
import androidx.compose.ui.platform.LocalContext
import androidx.compose.foundation.layout.PaddingValues
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import java.io.File
import kotlin.math.roundToInt

@OptIn(ExperimentalMaterial3Api::class)
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
    var showLibrary by remember { mutableStateOf(false) }
    var showQueue by remember { mutableStateOf(false) }

    val browser = remember { LibraryBrowser() }
    val browseItems = remember { mutableStateListOf<BrowseItem>() }
    var libInfo by remember { mutableStateOf("—") }
    var libPath by remember { mutableStateOf("文件夹") }
    val queue = remember { mutableStateListOf<String>() }
    var queueIndex by remember { mutableStateOf<Int?>(null) }

    fun refreshBrowseList() {
        browseItems.clear()
        browseItems.addAll(browser.currentItems())
        libPath = browser.currentPathLabel()
        libInfo = "${browser.mounts.size} 个源 · ${browser.allTracks.size} 首"
    }

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

    suspend fun reloadQueue() {
        try {
            val q = api.getQueue()
            queue.clear()
            queue.addAll(q.paths)
            queueIndex = q.index
        } catch (_: Exception) {
        }
    }

    fun runCmd(
        label: String,
        refreshQueue: Boolean = false,
        block: suspend () -> StatusResponse,
    ) {
        scope.launch {
            try {
                applyStatus(block())
                if (refreshQueue) reloadQueue()
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

            val mounts = try {
                api.getMounts().mounts
            } catch (_: Exception) {
                emptyList()
            }
            val tracks = api.getAllTracks()
            browser.reset(mounts, tracks)
            refreshBrowseList()
        } catch (e: Exception) {
            connected = false
            log = "连接失败: ${e.message}"
        }
    }

    fun onBrowseClick(item: BrowseItem) {
        when (item.kind) {
            BrowseKind.Mount, BrowseKind.Folder -> {
                browser.enter(item)
                refreshBrowseList()
            }
            BrowseKind.Track -> {
                val (paths, start) = browser.playPathsForTrack(item)
                if (paths.isEmpty()) {
                    log = "没有可播放的音频"
                    return
                }
                runCmd("播放", refreshQueue = true) { api.setQueue(paths, start) }
            }
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
    val coverKey = status.path ?: ""
    val coverUrl = remember(coverKey, api.baseUrl) { api.coverUrl(status.path) }
    val ctx = LocalContext.current

    Column(
        Modifier
            .fillMaxSize()
            .padding(horizontal = 20.dp)
            .padding(top = 8.dp, bottom = 12.dp),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        // 顶栏
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                "RKSonos",
                fontWeight = FontWeight.SemiBold,
                fontSize = 16.sp,
                color = TextPrimary,
            )
            IconButton(onClick = {
                settingsUrl = api.baseUrl
                showSettings = true
            }) {
                Icon(Icons.Default.Settings, contentDescription = "设置", tint = Muted)
            }
        }

        Spacer(Modifier.height(8.dp))

        // 大封面（参考主流播放器）
        Card(
            Modifier
                .fillMaxWidth(0.88f)
                .aspectRatio(1f),
            shape = RoundedCornerShape(16.dp),
            colors = CardDefaults.cardColors(containerColor = Panel),
            elevation = CardDefaults.cardElevation(defaultElevation = 6.dp)
        ) {
            SubcomposeAsyncImage(
                model = ImageRequest.Builder(ctx)
                    .data(coverUrl)
                    .crossfade(true)
                    .build(),
                contentDescription = "封面",
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize(),
                loading = {
                    CoverPlaceholder()
                },
                error = {
                    CoverPlaceholder()
                }
            )
        }

        Spacer(Modifier.height(22.dp))

        // 标题
        Text(
            title,
            fontSize = 22.sp,
            fontWeight = FontWeight.Bold,
            color = TextPrimary,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.fillMaxWidth()
        )
        Spacer(Modifier.height(6.dp))
        Text(
            status.path?.substringBeforeLast('/')?.substringAfterLast('/') ?: " ",
            fontSize = 14.sp,
            color = Muted,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.fillMaxWidth()
        )

        Spacer(Modifier.height(20.dp))

        // 进度
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
            modifier = Modifier.fillMaxWidth(),
            colors = SliderDefaults.colors(
                thumbColor = TextPrimary,
                activeTrackColor = TextPrimary,
                inactiveTrackColor = Muted.copy(alpha = 0.35f)
            )
        )
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text(fmtTime(progressValue.toDouble()), color = Muted, fontSize = 12.sp)
            Text(
                if (dur > 0.5f) "-${fmtTime((dur - progressValue).toDouble().coerceAtLeast(0.0))}"
                else "0:00",
                color = Muted,
                fontSize = 12.sp
            )
        }

        Spacer(Modifier.height(18.dp))

        // 控制条：拉开间距，参考截图
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceEvenly,
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(
                onClick = {
                    val next = when (mode) {
                        "normal" -> "shuffle"
                        "shuffle" -> "repeat_all"
                        "repeat_all" -> "repeat_one"
                        else -> "normal"
                    }
                    runCmd("模式") { api.setMode(next) }
                },
                modifier = Modifier.size(48.dp)
            ) {
                val (icon, alpha) = when (mode) {
                    "shuffle" -> Icons.Default.Shuffle to 1f
                    "repeat_one" -> Icons.Default.RepeatOne to 1f
                    "repeat_all" -> Icons.Default.Repeat to 1f
                    else -> Icons.Default.Shuffle to 0.35f
                }
                Icon(icon, contentDescription = "模式", tint = TextPrimary.copy(alpha = alpha), modifier = Modifier.size(26.dp))
            }
            IconButton(onClick = { runCmd("上一首", refreshQueue = true) { api.prev() } }, modifier = Modifier.size(52.dp)) {
                Icon(Icons.Default.SkipPrevious, contentDescription = "上一首", tint = TextPrimary, modifier = Modifier.size(36.dp))
            }
            Box(
                Modifier
                    .size(68.dp)
                    .clip(CircleShape)
                    .background(TextPrimary)
                    .clickable {
                        if (playing) runCmd("暂停") { api.pause() }
                        else runCmd("播放") { api.resume() }
                    },
                contentAlignment = Alignment.Center
            ) {
                Icon(
                    if (playing) Icons.Default.Pause else Icons.Default.PlayArrow,
                    contentDescription = "播放/暂停",
                    tint = Color(0xFF1E1E2E),
                    modifier = Modifier.size(34.dp)
                )
            }
            IconButton(onClick = { runCmd("下一首", refreshQueue = true) { api.next() } }, modifier = Modifier.size(52.dp)) {
                Icon(Icons.Default.SkipNext, contentDescription = "下一首", tint = TextPrimary, modifier = Modifier.size(36.dp))
            }
            IconButton(
                onClick = {
                    val next = when (mode) {
                        "normal" -> "repeat_all"
                        "repeat_all" -> "repeat_one"
                        "repeat_one" -> "normal"
                        else -> "repeat_all"
                    }
                    runCmd("模式") { api.setMode(next) }
                },
                modifier = Modifier.size(48.dp)
            ) {
                val (icon, alpha) = when (mode) {
                    "repeat_one" -> Icons.Default.RepeatOne to 1f
                    "repeat_all" -> Icons.Default.Repeat to 1f
                    else -> Icons.Default.Repeat to 0.35f
                }
                Icon(icon, contentDescription = "循环", tint = TextPrimary.copy(alpha = alpha), modifier = Modifier.size(26.dp))
            }
        }

        Spacer(Modifier.height(16.dp))

        // 音量
        Row(
            Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Icon(Icons.Default.VolumeUp, contentDescription = null, tint = Muted, modifier = Modifier.size(20.dp))
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
                modifier = Modifier.weight(1f).padding(horizontal = 8.dp),
                colors = SliderDefaults.colors(
                    thumbColor = TextPrimary,
                    activeTrackColor = TextPrimary,
                    inactiveTrackColor = Muted.copy(alpha = 0.35f)
                )
            )
            Text("${volumeValue.roundToInt()}", color = Muted, fontSize = 12.sp, modifier = Modifier.width(28.dp))
        }

        Spacer(Modifier.weight(1f))

        val qiLabel = when {
            queue.isEmpty() -> "0"
            queueIndex != null && queueIndex!! >= 0 -> "${queueIndex!! + 1}/${queue.size}"
            else -> "${queue.size}"
        }
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            OutlinedButton(
                onClick = { showLibrary = true },
                modifier = Modifier.weight(1f).height(48.dp),
                shape = RoundedCornerShape(12.dp),
                colors = ButtonDefaults.outlinedButtonColors(contentColor = TextPrimary)
            ) {
                Icon(Icons.Default.LibraryMusic, contentDescription = null, tint = Accent)
                Spacer(Modifier.width(8.dp))
                Column {
                    Text("音乐库", fontWeight = FontWeight.SemiBold, color = TextPrimary)
                    Text(libInfo, fontSize = 11.sp, color = Muted)
                }
            }
            OutlinedButton(
                onClick = { showQueue = true },
                modifier = Modifier.weight(1f).height(48.dp),
                shape = RoundedCornerShape(12.dp),
                colors = ButtonDefaults.outlinedButtonColors(contentColor = TextPrimary)
            ) {
                Icon(Icons.Default.QueueMusic, contentDescription = null, tint = Accent)
                Spacer(Modifier.width(8.dp))
                Column {
                    Text("播放列表", fontWeight = FontWeight.SemiBold, color = TextPrimary)
                    Text(qiLabel, fontSize = 11.sp, color = Muted)
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
                status.state,
                color = Muted,
                fontSize = 12.sp,
                modifier = Modifier.weight(1f),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis
            )
        }
    }

    if (showLibrary) {
        ModalBottomSheet(
            onDismissRequest = { showLibrary = false },
            sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
            containerColor = Panel,
        ) {
            Column(
                Modifier
                    .fillMaxWidth()
                    .fillMaxHeight(0.85f)
                    .padding(horizontal = 16.dp)
                    .padding(bottom = 24.dp)
            ) {
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text("音乐库", fontWeight = FontWeight.SemiBold, fontSize = 18.sp, color = TextPrimary)
                    Text(libInfo, color = Muted, fontSize = 12.sp)
                }
                Spacer(Modifier.height(8.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (browser.canGoBack()) {
                        Text(
                            "← 返回",
                            color = Accent,
                            fontSize = 14.sp,
                            modifier = Modifier
                                .clickable {
                                    browser.goBack()
                                    refreshBrowseList()
                                }
                                .padding(end = 12.dp, top = 6.dp, bottom = 6.dp)
                        )
                    }
                    Text(
                        libPath,
                        color = Muted,
                        fontSize = 13.sp,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis
                    )
                }
                Spacer(Modifier.height(8.dp))
                if (browseItems.isEmpty()) {
                    Text(
                        if (browser.mounts.isEmpty())
                            "请先在 PC 客户端「音乐库设置」添加音源"
                        else
                            "此文件夹为空",
                        color = Muted,
                        fontSize = 13.sp,
                        modifier = Modifier.padding(top = 16.dp)
                    )
                }
                LazyColumn(Modifier.fillMaxSize()) {
                    itemsIndexed(
                        browseItems,
                        key = { i, it -> "${it.kind}:${it.navKey}:${it.trackPath}:$i" }
                    ) { _, item ->
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .clickable { onBrowseClick(item) }
                                .padding(vertical = 12.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text(item.icon, fontSize = 16.sp)
                            Spacer(Modifier.width(10.dp))
                            Text(
                                item.title,
                                color = TextPrimary,
                                fontWeight = if (item.kind == BrowseKind.Track)
                                    FontWeight.SemiBold else FontWeight.Normal,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f)
                            )
                        }
                    }
                }
            }
        }
    }

    if (showQueue) {
        ModalBottomSheet(
            onDismissRequest = { showQueue = false },
            sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
            containerColor = Panel,
        ) {
            val qi = queueIndex
            val countText = when {
                queue.isEmpty() -> "— / 0"
                qi != null && qi >= 0 -> "第 ${qi + 1} / ${queue.size} 首"
                else -> "— / ${queue.size}"
            }
            Column(
                Modifier
                    .fillMaxWidth()
                    .fillMaxHeight(0.85f)
                    .padding(horizontal = 16.dp)
                    .padding(bottom = 24.dp)
            ) {
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text("播放列表", fontWeight = FontWeight.SemiBold, fontSize = 18.sp, color = TextPrimary)
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
                                    runCmd("播放", refreshQueue = true) {
                                        api.setQueue(queue.toList(), i)
                                    }
                                }
                                .padding(vertical = 12.dp, horizontal = 6.dp),
                            color = if (cur) Accent else TextPrimary,
                            fontWeight = if (cur) FontWeight.SemiBold else FontWeight.Normal,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                    }
                }
                TextButton(onClick = {
                    runCmd("清空", refreshQueue = true) { api.queueClear() }
                }) {
                    Text("清空列表")
                }
            }
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

@Composable
private fun CoverPlaceholder() {
    Box(
        Modifier
            .fillMaxSize()
            .background(Panel),
        contentAlignment = Alignment.Center
    ) {
        Icon(
            Icons.Default.LibraryMusic,
            contentDescription = null,
            tint = Muted.copy(alpha = 0.5f),
            modifier = Modifier.size(72.dp)
        )
    }
}

private fun fmtTime(sec: Double): String {
    val s = if (sec.isNaN() || sec < 0) 0 else sec.toInt()
    val m = s / 60
    val r = s % 60
    return "%d:%02d".format(m, r)
}
