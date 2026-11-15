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
