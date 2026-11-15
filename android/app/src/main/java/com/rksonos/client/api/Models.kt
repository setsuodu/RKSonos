package com.rksonos.client.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class StatusResponse(
    val state: String = "idle",
    val path: String? = null,
    val volume: Int = 50,
    @SerialName("audio_device") val audioDevice: String? = null,
    @SerialName("queue_len") val queueLen: Int = 0,
    @SerialName("queue_index") val queueIndex: Int? = null,
    val mode: String = "normal",
    val position: Double? = null,
    val duration: Double? = null,
    @SerialName("server_version") val serverVersion: String? = null,
)

@Serializable
data class Track(
    val id: Long = 0,
    val path: String = "",
    @SerialName("rel_path") val relPath: String? = null,
    val name: String = "",
    @SerialName("mount_id") val mountId: String? = null,
) {
    val displayName: String
        get() = name.ifBlank { path.substringAfterLast('/').ifBlank { path } }
}

@Serializable
data class QueueResponse(
    val paths: List<String> = emptyList(),
    val index: Int? = null,
)

@Serializable
data class LibraryStatus(
    val count: Int = 0,
    val scanning: Boolean = false,
)

@Serializable
data class MountView(
    val id: String = "",
    val uri: String = "",
    val label: String = "",
    val username: String? = null,
    @SerialName("mount_point") val mountPoint: String? = null,
    val enabled: Boolean = true,
    val mounted: Boolean = false,
)

@Serializable
data class MountListResponse(
    val mounts: List<MountView> = emptyList(),
    val gio: Boolean = false,
)

@Serializable
data class ApiError(val error: String = "")

data class DiscoveredDevice(
    val name: String,
    val host: String,
    val port: Int = 8080,
    val source: String = "",
) {
    val address: String get() = "http://$host:$port"
    val display: String
        get() = if (name.isBlank() || name == "RKSonos") address else "$name  ·  $host"
}

enum class BrowseKind { Mount, Folder, Track }

/** 与 WPF BrowseItem 对齐的层级浏览节点 */
data class BrowseItem(
    val kind: BrowseKind,
    val title: String,
    /** 导航键：mountId 或 mountId/folder/... */
    val navKey: String = "",
    val trackPath: String? = null,
    val track: Track? = null,
) {
    val isNavigable: Boolean get() = kind == BrowseKind.Mount || kind == BrowseKind.Folder
    val icon: String
        get() = when (kind) {
            BrowseKind.Mount -> "🔊"
            BrowseKind.Folder -> "📁"
            BrowseKind.Track -> "🎵"
        }
}
