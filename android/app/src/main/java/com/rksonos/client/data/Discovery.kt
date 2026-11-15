package com.rksonos.client.data

import com.rksonos.client.api.DiscoveredDevice
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import java.net.Inet4Address
import java.net.NetworkInterface
import java.net.Socket
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit

object Discovery {
    private val http = OkHttpClient.Builder()
        .connectTimeout(400, TimeUnit.MILLISECONDS)
        .readTimeout(600, TimeUnit.MILLISECONDS)
        .callTimeout(900, TimeUnit.MILLISECONDS)
        .build()

    suspend fun scan(
        hints: List<String> = emptyList(),
        onFound: (DiscoveredDevice) -> Unit = {},
    ): List<DiscoveredDevice> = withContext(Dispatchers.IO) {
        val found = ConcurrentHashMap<String, DiscoveredDevice>()

        fun publish(d: DiscoveredDevice) {
            val key = "${d.host}:${d.port}"
            if (found.putIfAbsent(key, d) == null) onFound(d)
        }

        for (h in hints) {
            tryHealth(h)?.let { publish(it) }
        }

        val prefixes = localIpv4Prefixes().take(1)
        if (prefixes.isEmpty()) return@withContext found.values.toList()

        val prefix = prefixes[0]
        val sem = Semaphore(64)
        coroutineScope {
            (1..254).map { i ->
                async {
                    sem.withPermit {
                        val ip = "$prefix.$i"
                        if (found.containsKey("$ip:8080")) return@withPermit
                        if (!tcpOpen(ip, 8080, 150)) return@withPermit
                        tryHealth("http://$ip:8080")?.let { publish(it) }
                    }
                }
            }.awaitAll()
        }
        found.values.sortedBy { it.host }
    }

    fun tryHealth(urlOrHost: String): DiscoveredDevice? {
        return try {
            var raw = urlOrHost.trim().trimEnd('/')
            if (raw.isBlank()) return null
            if (!raw.startsWith("http://", true) && !raw.startsWith("https://", true)) {
                raw = "http://$raw"
            }
            val uri = java.net.URI(raw)
            val host = uri.host ?: return null
            val port = if (uri.port > 0) uri.port else 8080
            val req = Request.Builder().url("$raw/health").get().build()
            http.newCall(req).execute().use { resp ->
                if (!resp.isSuccessful) return null
                val body = resp.body?.string()?.trim().orEmpty()
                if (!body.equals("ok", true)) return null
                DiscoveredDevice(name = "RKSonos", host = host, port = port, source = "lan")
            }
        } catch (_: Exception) {
            null
        }
    }

    private fun tcpOpen(host: String, port: Int, timeoutMs: Int): Boolean {
        return try {
            Socket().use { s ->
                s.connect(java.net.InetSocketAddress(host, port), timeoutMs)
                true
            }
        } catch (_: Exception) {
            false
        }
    }

    private fun localIpv4Prefixes(): List<String> {
        val out = linkedSetOf<String>()
        try {
            val en = NetworkInterface.getNetworkInterfaces() ?: return emptyList()
            for (ni in en) {
                if (!ni.isUp || ni.isLoopback) continue
                for (addr in ni.inetAddresses) {
                    if (addr !is Inet4Address) continue
                    val s = addr.hostAddress ?: continue
                    if (s.startsWith("127.") || s.startsWith("169.254.")) continue
                    val parts = s.split('.')
                    if (parts.size == 4) out += "${parts[0]}.${parts[1]}.${parts[2]}"
                }
            }
        } catch (_: Exception) {
        }
        return out.toList()
    }
}
