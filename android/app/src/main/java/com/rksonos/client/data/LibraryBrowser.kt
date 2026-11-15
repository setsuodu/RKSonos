package com.rksonos.client.data

import com.rksonos.client.api.BrowseItem
import com.rksonos.client.api.BrowseKind
import com.rksonos.client.api.MountView
import com.rksonos.client.api.Track

/**
 * 对齐 WPF：挂载源 → 文件夹 → 曲目 的客户端侧层级浏览。
 * 数据来自 mounts + library 索引（rel_path / mount_id）。
 */
class LibraryBrowser {
    var mounts: List<MountView> = emptyList()
        private set
    var allTracks: List<Track> = emptyList()
        private set

    /** 导航栈：空=根（各挂载）；非空为当前 navKey */
    private val navStack = mutableListOf<String>()

    fun reset(mounts: List<MountView>, tracks: List<Track>) {
        this.mounts = mounts
        this.allTracks = tracks
        navStack.clear()
    }

    fun depth(): Int = navStack.size

    fun currentPathLabel(): String {
        if (navStack.isEmpty()) return "文件夹"
        return "//" + navStack.last().replace('\\', '/')
    }

    fun canGoBack(): Boolean = navStack.isNotEmpty()

    fun goBack() {
        if (navStack.isNotEmpty()) navStack.removeAt(navStack.lastIndex)
    }

    fun enter(item: BrowseItem) {
        if (!item.isNavigable) return
        navStack.add(item.navKey)
    }

    fun currentItems(): List<BrowseItem> {
        return if (navStack.isEmpty()) rootItems() else navItems(navStack.last())
    }

    private fun rootItems(): List<BrowseItem> {
        if (mounts.isEmpty()) return emptyList()
        return mounts.map { m ->
            var title = m.uri
            if (title.startsWith("smb://", ignoreCase = true)) {
                title = "//" + title.removePrefix("smb://")
            }
            if (title.isBlank()) title = m.label.ifBlank { m.id }
            BrowseItem(BrowseKind.Mount, title = title, navKey = m.id)
        }
    }

    private fun navItems(navKey: String): List<BrowseItem> {
        val prefix = navKey.trim().trimEnd('/')
        val folders = sortedSetOf(String.CASE_INSENSITIVE_ORDER)
        val files = mutableListOf<Track>()

        for (tr in allTracks) {
            val rest = relativeUnder(tr, prefix) ?: continue
            if (rest.isEmpty()) continue
            val slash = rest.indexOf('/')
            if (slash < 0) files += tr
            else folders += rest.substring(0, slash)
        }

        val out = mutableListOf<BrowseItem>()
        for (folder in folders) {
            out += BrowseItem(
                kind = BrowseKind.Folder,
                title = folder,
                navKey = "$prefix/$folder",
            )
        }
        for (tr in files.sortedBy { it.displayName.lowercase() }) {
            out += BrowseItem(
                kind = BrowseKind.Track,
                title = tr.displayName,
                navKey = prefix,
                trackPath = tr.path,
                track = tr,
            )
        }
        return out
    }

    /**
     * 相对路径：在 prefix 下的剩余段。prefix 通常是 mountId 或 mountId/dir。
     */
    private fun relativeUnder(tr: Track, prefix: String): String? {
        val rel = (tr.relPath ?: "").replace('\\', '/').trim('/')
        if (rel.isEmpty()) return null

        if (rel.startsWith("$prefix/", ignoreCase = true)) {
            return rel.substring(prefix.length + 1)
        }
        // mount 根：prefix == mountId，rel 可能不含 mountId 前缀
        if (tr.mountId != null && tr.mountId.equals(prefix, ignoreCase = true)) {
            if (rel.equals(prefix, ignoreCase = true)) return null
            if (rel.startsWith("$prefix/", ignoreCase = true)) {
                return rel.substring(prefix.length + 1)
            }
            return rel
        }
        return null
    }

    /** 播放曲目：同层全部音频，从点中这首开始（对齐 WPF） */
    fun playPathsForTrack(item: BrowseItem): Pair<List<String>, Int> {
        val path = item.trackPath ?: return emptyList<String>() to 0
        val parent = item.navKey.trim().trimEnd('/')
        val paths = tracksUnder(parent, immediateOnly = true).map { it.path }
        if (paths.isEmpty()) return listOf(path) to 0
        val start = paths.indexOfFirst { it.equals(path, ignoreCase = true) }.coerceAtLeast(0)
        return paths to start
    }

    /** 播放文件夹/源：该节点下全部音频（含子目录） */
    fun playPathsForFolder(item: BrowseItem): List<String> {
        val prefix = item.navKey.trim().trimEnd('/')
        return tracksUnder(prefix, immediateOnly = false).map { it.path }
    }

    private fun tracksUnder(prefix: String, immediateOnly: Boolean): List<Track> {
        val p = prefix.trim().trimEnd('/')
        val out = mutableListOf<Track>()
        for (tr in allTracks) {
            val rest = relativeUnder(tr, p) ?: continue
            if (rest.isEmpty()) continue
            if (immediateOnly && rest.contains('/')) continue
            out += tr
        }
        return out.sortedBy { it.displayName.lowercase() }
    }
}
