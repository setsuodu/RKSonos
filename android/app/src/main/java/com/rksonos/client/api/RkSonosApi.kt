package com.rksonos.client.api

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.TimeUnit

class RkSonosApi(
    baseUrl: String = "http://127.0.0.1:8080",
) {
    private val json = Json {
        ignoreUnknownKeys = true
        isLenient = true
    }
    private val client = OkHttpClient.Builder()
        .connectTimeout(3, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .writeTimeout(15, TimeUnit.SECONDS)
        .build()

    @Volatile
    var baseUrl: String = normalize(baseUrl)
        set(value) {
            field = normalize(value)
        }

    private fun normalize(v: String): String {
        var s = v.trim().trimEnd('/')
        if (s.isBlank()) s = "http://127.0.0.1:8080"
        if (!s.startsWith("http://", true) && !s.startsWith("https://", true)) {
            s = "http://$s"
        }
        return s
    }

    private fun url(path: String): String =
        "${baseUrl.trimEnd('/')}/${path.trimStart('/')}"

    private suspend fun <T> http(block: () -> T): T =
        withContext(Dispatchers.IO) { block() }

    suspend fun health(): String = http {
        val req = Request.Builder().url(url("health")).get().build()
        client.newCall(req).execute().use { resp ->
            resp.body?.string()?.trim().orEmpty()
        }
    }

    suspend fun getStatus(): StatusResponse = http {
        getJson("api/v1/status")
    }

    suspend fun pause(): StatusResponse = postEmpty("api/v1/pause")
    suspend fun resume(): StatusResponse = postEmpty("api/v1/resume")
    suspend fun stop(): StatusResponse = postEmpty("api/v1/stop")
    suspend fun next(): StatusResponse = postEmpty("api/v1/next")
    suspend fun prev(): StatusResponse = postEmpty("api/v1/prev")

    suspend fun setVolume(volume: Int): StatusResponse = http {
        postJson("api/v1/volume", buildJsonObject { put("volume", volume.coerceIn(0, 100)) })
    }

    suspend fun setMode(mode: String): StatusResponse = http {
        postJson("api/v1/mode", buildJsonObject { put("mode", mode) })
    }

    suspend fun seek(position: Double): StatusResponse = http {
        postJson("api/v1/seek", buildJsonObject { put("position", position) })
    }

    suspend fun getQueue(): QueueResponse = http {
        getJson("api/v1/queue")
    }

    suspend fun setQueue(paths: List<String>, start: Int = 0): StatusResponse = http {
        val payload = buildJsonObject {
            put(
                "paths",
                kotlinx.serialization.json.JsonArray(
                    paths.map { kotlinx.serialization.json.JsonPrimitive(it) }
                )
            )
            put("start", start.coerceAtLeast(0))
        }
        postJson("api/v1/queue", payload)
    }

    suspend fun queueClear(): StatusResponse = postEmpty("api/v1/queue/clear")

    suspend fun getLibrary(q: String? = null, limit: Int = 500, offset: Int = 0): List<Track> = http {
        val qs = buildString {
            append("api/v1/library?limit=$limit&offset=$offset")
            if (!q.isNullOrBlank()) append("&q=${java.net.URLEncoder.encode(q, "UTF-8")}")
        }
        getJson(qs)
    }

    suspend fun libraryPlay(path: String): StatusResponse = http {
        postJson("api/v1/library/play", buildJsonObject { put("path", path) })
    }

    private inline fun <reified T> getJson(path: String): T {
        val req = Request.Builder().url(url(path)).get().build()
        client.newCall(req).execute().use { resp ->
            val body = resp.body?.string().orEmpty()
            if (!resp.isSuccessful) throw ApiException(parseError(body), resp.code)
            return json.decodeFromString(body)
        }
    }

    private suspend fun postEmpty(path: String): StatusResponse = http {
        val req = Request.Builder()
            .url(url(path))
            .post(ByteArray(0).toRequestBody(null))
            .build()
        client.newCall(req).execute().use { resp ->
            val body = resp.body?.string().orEmpty()
            if (!resp.isSuccessful) throw ApiException(parseError(body), resp.code)
            json.decodeFromString(body)
        }
    }

    private inline fun <reified T> postJson(path: String, payload: kotlinx.serialization.json.JsonObject): T {
        val media = "application/json; charset=utf-8".toMediaType()
        val req = Request.Builder()
            .url(url(path))
            .post(json.encodeToString(payload).toRequestBody(media))
            .build()
        client.newCall(req).execute().use { resp ->
            val body = resp.body?.string().orEmpty()
            if (!resp.isSuccessful) throw ApiException(parseError(body), resp.code)
            return json.decodeFromString(body)
        }
    }

    private fun parseError(body: String): String {
        return try {
            json.decodeFromString<ApiError>(body).error.ifBlank { body }
        } catch (_: Exception) {
            body.ifBlank { "request failed" }
        }
    }
}

class ApiException(message: String, val code: Int = 0) : Exception(message)
