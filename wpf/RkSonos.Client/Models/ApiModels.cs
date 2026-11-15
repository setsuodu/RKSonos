using System.Text.Json.Serialization;

namespace RkSonos.Client.Models;

public sealed class StatusResponse
{
    [JsonPropertyName("state")]
    public string State { get; set; } = "idle";

    [JsonPropertyName("path")]
    public string? Path { get; set; }

    [JsonPropertyName("volume")]
    public int Volume { get; set; }

    [JsonPropertyName("audio_device")]
    public string? AudioDevice { get; set; }

    [JsonPropertyName("queue_len")]
    public int QueueLen { get; set; }

    [JsonPropertyName("queue_index")]
    public int? QueueIndex { get; set; }

    [JsonPropertyName("mode")]
    public string Mode { get; set; } = "normal";

    /// seconds
    [JsonPropertyName("position")]
    public double? Position { get; set; }

    /// seconds
    [JsonPropertyName("duration")]
    public double? Duration { get; set; }
}

public sealed class Track
{
    [JsonPropertyName("id")]
    public long Id { get; set; }

    [JsonPropertyName("path")]
    public string Path { get; set; } = "";

    [JsonPropertyName("rel_path")]
    public string RelPath { get; set; } = "";

    [JsonPropertyName("name")]
    public string Name { get; set; } = "";

    [JsonPropertyName("size")]
    public long Size { get; set; }

    [JsonPropertyName("mtime")]
    public long Mtime { get; set; }

    [JsonPropertyName("mount_id")]
    public string? MountId { get; set; }

    public string DisplayName => string.IsNullOrWhiteSpace(Name) ? System.IO.Path.GetFileName(Path) : Name;
}

public sealed class RootInfo
{
    [JsonPropertyName("id")]
    public string Id { get; set; } = "";

    [JsonPropertyName("path")]
    public string Path { get; set; } = "";
}

public sealed class LibraryStatus
{
    // legacy single root (old server)
    [JsonPropertyName("root")]
    public string Root { get; set; } = "";

    // multi-root (new server)
    [JsonPropertyName("roots")]
    public List<RootInfo> Roots { get; set; } = new();

    [JsonPropertyName("scanning")]
    public bool Scanning { get; set; }

    [JsonPropertyName("track_count")]
    public int TrackCount { get; set; }

    [JsonPropertyName("last_scan_ms")]
    public long LastScanMs { get; set; }

    [JsonPropertyName("last_error")]
    public string? LastError { get; set; }
}

public sealed class ScanResponse
{
    [JsonPropertyName("status")]
    public string Status { get; set; } = "";

    [JsonPropertyName("count")]
    public int Count { get; set; }

    [JsonPropertyName("took_ms")]
    public long TookMs { get; set; }
}

public sealed class QueueResponse
{
    [JsonPropertyName("paths")]
    public List<string> Paths { get; set; } = new();

    [JsonPropertyName("index")]
    public int? Index { get; set; }
}

public sealed class ApiError
{
    [JsonPropertyName("error")]
    public string Error { get; set; } = "";
}

public sealed class QueueItem
{
    public int Index { get; set; }
    public string Path { get; set; } = "";
    public string Name => System.IO.Path.GetFileName(Path);
    public bool IsCurrent { get; set; }
}

public sealed class MountView
{
    [JsonPropertyName("id")]
    public string Id { get; set; } = "";

    [JsonPropertyName("uri")]
    public string Uri { get; set; } = "";

    [JsonPropertyName("label")]
    public string Label { get; set; } = "";

    [JsonPropertyName("username")]
    public string? Username { get; set; }

    [JsonPropertyName("mount_point")]
    public string? MountPoint { get; set; }

    [JsonPropertyName("enabled")]
    public bool Enabled { get; set; }

    [JsonPropertyName("mounted")]
    public bool Mounted { get; set; }

    public string Display =>
        string.IsNullOrWhiteSpace(Label) ? Uri : $"{Label}  ({Uri})";

    public string StatusText =>
        Mounted ? "已挂载" : (Enabled ? "未挂载" : "禁用");
}

public sealed class MountListResponse
{
    [JsonPropertyName("mounts")]
    public List<MountView> Mounts { get; set; } = new();

    [JsonPropertyName("gio")]
    public bool Gio { get; set; }
}


public enum BrowseKind
{
    Mount,
    Folder,
    Track,
}

/// <summary>音乐库列表项：挂载 / 文件夹 / 歌曲</summary>
public sealed class BrowseItem
{
    public BrowseKind Kind { get; set; }
    public string Title { get; set; } = "";
    public string Subtitle { get; set; } = "";
    public string Icon => Kind switch
    {
        BrowseKind.Mount => "💿",
        BrowseKind.Folder => "📁",
        _ => "🎵",
    };

    /// <summary>导航用：挂载 id，或 mountId/相对目录前缀</summary>
    public string NavKey { get; set; } = "";

    /// <summary>仅 Track：服务器绝对路径</summary>
    public string? TrackPath { get; set; }

    public Track? Track { get; set; }
}
