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

    public string DisplayName => string.IsNullOrWhiteSpace(Name) ? System.IO.Path.GetFileName(Path) : Name;
}

public sealed class LibraryStatus
{
    [JsonPropertyName("root")]
    public string Root { get; set; } = "";

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
