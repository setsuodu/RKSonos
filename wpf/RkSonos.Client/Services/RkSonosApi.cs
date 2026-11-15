using System.Net.Http;
using System.Net.Http.Json;
using System.Text;
using System.Text.Json;
using RkSonos.Client.Models;

namespace RkSonos.Client.Services;

public sealed class RkSonosApi : IDisposable
{
    private static readonly JsonSerializerOptions JsonOpts = new()
    {
        PropertyNameCaseInsensitive = true,
    };

    private HttpClient _http = new() { Timeout = TimeSpan.FromSeconds(60) };
    private string _baseUrl = "http://127.0.0.1:8080";

    public string BaseUrl
    {
        get => _baseUrl;
        set
        {
            var v = (value ?? "").Trim().TrimEnd('/');
            if (string.IsNullOrWhiteSpace(v))
                v = "http://127.0.0.1:8080";
            if (!v.StartsWith("http://", StringComparison.OrdinalIgnoreCase) &&
                !v.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
                v = "http://" + v;
            _baseUrl = v;
            _http.Dispose();
            _http = new HttpClient { Timeout = TimeSpan.FromSeconds(60), BaseAddress = new Uri(_baseUrl + "/") };
        }
    }

    public RkSonosApi()
    {
        BaseUrl = "http://127.0.0.1:8080";
    }

    public async Task<string> HealthAsync(CancellationToken ct = default)
    {
        var s = await _http.GetStringAsync("health", ct);
        return s.Trim();
    }

    public Task<StatusResponse?> GetStatusAsync(CancellationToken ct = default)
        => _http.GetFromJsonAsync<StatusResponse>("api/v1/status", JsonOpts, ct);

    public async Task<StatusResponse?> PostEmptyAsync(string path, CancellationToken ct = default)
    {
        using var resp = await _http.PostAsync(path, content: null, ct);
        return await ReadStatusOrThrow(resp, ct);
    }

    public async Task<StatusResponse?> PlayAsync(string filePath, CancellationToken ct = default)
    {
        using var resp = await _http.PostAsJsonAsync("api/v1/play", new { path = filePath }, ct);
        return await ReadStatusOrThrow(resp, ct);
    }

    public Task<StatusResponse?> PauseAsync(CancellationToken ct = default)
        => PostEmptyAsync("api/v1/pause", ct);

    public Task<StatusResponse?> ResumeAsync(CancellationToken ct = default)
        => PostEmptyAsync("api/v1/resume", ct);

    public Task<StatusResponse?> StopAsync(CancellationToken ct = default)
        => PostEmptyAsync("api/v1/stop", ct);

    public Task<StatusResponse?> NextAsync(CancellationToken ct = default)
        => PostEmptyAsync("api/v1/next", ct);

    public Task<StatusResponse?> PrevAsync(CancellationToken ct = default)
        => PostEmptyAsync("api/v1/prev", ct);

    public async Task<StatusResponse?> SetVolumeAsync(int volume, CancellationToken ct = default)
    {
        using var resp = await _http.PostAsJsonAsync("api/v1/volume", new { volume }, ct);
        return await ReadStatusOrThrow(resp, ct);
    }

    public async Task<StatusResponse?> SetModeAsync(string mode, CancellationToken ct = default)
    {
        using var resp = await _http.PostAsJsonAsync("api/v1/mode", new { mode }, ct);
        return await ReadStatusOrThrow(resp, ct);
    }

    public async Task<QueueResponse?> GetQueueAsync(CancellationToken ct = default)
        => await _http.GetFromJsonAsync<QueueResponse>("api/v1/queue", JsonOpts, ct);

    public async Task<StatusResponse?> SetQueueAsync(IEnumerable<string> paths, int start = 0, CancellationToken ct = default)
    {
        using var resp = await _http.PostAsJsonAsync("api/v1/queue", new { paths = paths.ToList(), start }, ct);
        return await ReadStatusOrThrow(resp, ct);
    }

    public async Task<StatusResponse?> QueueAddAsync(IEnumerable<string> paths, CancellationToken ct = default)
    {
        using var resp = await _http.PostAsJsonAsync("api/v1/queue/add", new { paths = paths.ToList() }, ct);
        return await ReadStatusOrThrow(resp, ct);
    }

    public async Task<StatusResponse?> QueueRemoveAsync(int index, CancellationToken ct = default)
    {
        using var resp = await _http.PostAsJsonAsync("api/v1/queue/remove", new { index }, ct);
        return await ReadStatusOrThrow(resp, ct);
    }

    public Task<StatusResponse?> QueueClearAsync(CancellationToken ct = default)
        => PostEmptyAsync("api/v1/queue/clear", ct);

    public async Task<ScanResponse?> LibraryScanAsync(CancellationToken ct = default)
    {
        using var resp = await _http.PostAsync("api/v1/library/scan", content: null, ct);
        if (!resp.IsSuccessStatusCode)
            throw new ApiException(await ReadError(resp, ct), (int)resp.StatusCode);
        return await resp.Content.ReadFromJsonAsync<ScanResponse>(JsonOpts, ct);
    }

    public Task<LibraryStatus?> GetLibraryStatusAsync(CancellationToken ct = default)
        => _http.GetFromJsonAsync<LibraryStatus>("api/v1/library/status", JsonOpts, ct);

    public async Task<List<Track>> GetLibraryAsync(string? q = null, int limit = 200, int offset = 0, CancellationToken ct = default)
    {
        var qs = new StringBuilder("api/v1/library?");
        qs.Append("limit=").Append(limit);
        qs.Append("&offset=").Append(offset);
        if (!string.IsNullOrWhiteSpace(q))
            qs.Append("&q=").Append(Uri.EscapeDataString(q));
        var list = await _http.GetFromJsonAsync<List<Track>>(qs.ToString(), JsonOpts, ct);
        return list ?? new List<Track>();
    }

    public async Task<StatusResponse?> LibraryPlayAsync(string? path = null, long? id = null, CancellationToken ct = default)
    {
        object body = path != null ? new { path } : new { id };
        using var resp = await _http.PostAsJsonAsync("api/v1/library/play", body, ct);
        return await ReadStatusOrThrow(resp, ct);
    }

    private static async Task<StatusResponse?> ReadStatusOrThrow(HttpResponseMessage resp, CancellationToken ct)
    {
        if (!resp.IsSuccessStatusCode)
            throw new ApiException(await ReadError(resp, ct), (int)resp.StatusCode);
        return await resp.Content.ReadFromJsonAsync<StatusResponse>(JsonOpts, ct);
    }

    private static async Task<string> ReadError(HttpResponseMessage resp, CancellationToken ct)
    {
        try
        {
            var err = await resp.Content.ReadFromJsonAsync<ApiError>(JsonOpts, ct);
            if (!string.IsNullOrWhiteSpace(err?.Error))
                return err!.Error;
        }
        catch { /* ignore */ }
        var raw = await resp.Content.ReadAsStringAsync(ct);
        return string.IsNullOrWhiteSpace(raw) ? resp.ReasonPhrase ?? "request failed" : raw;
    }


    // ----- mounts (client-driven SMB via gio) -----

    public async Task<MountListResponse?> GetMountsAsync(CancellationToken ct = default)
        => await _http.GetFromJsonAsync<MountListResponse>("api/v1/mounts", JsonOpts, ct);

    public async Task<MountView?> AddMountAsync(
        string uri,
        string? label = null,
        string? id = null,
        string? username = null,
        string? password = null,
        bool mountNow = true,
        CancellationToken ct = default)
    {
        var body = new Dictionary<string, object?>
        {
            ["uri"] = uri,
            ["mount_now"] = mountNow,
        };
        if (!string.IsNullOrWhiteSpace(label)) body["label"] = label;
        if (!string.IsNullOrWhiteSpace(id)) body["id"] = id;
        if (!string.IsNullOrWhiteSpace(username)) body["username"] = username;
        if (!string.IsNullOrWhiteSpace(password)) body["password"] = password;

        using var resp = await _http.PostAsJsonAsync("api/v1/mounts", body, ct);
        if (!resp.IsSuccessStatusCode)
            throw new ApiException(await ReadError(resp, ct), (int)resp.StatusCode);
        return await resp.Content.ReadFromJsonAsync<MountView>(JsonOpts, ct);
    }

    public async Task DeleteMountAsync(string id, CancellationToken ct = default)
    {
        using var resp = await _http.DeleteAsync("api/v1/mounts/" + Uri.EscapeDataString(id), ct);
        if (!resp.IsSuccessStatusCode)
            throw new ApiException(await ReadError(resp, ct), (int)resp.StatusCode);
    }

    public async Task<MountView?> RemountAsync(string id, CancellationToken ct = default)
    {
        using var resp = await _http.PostAsync("api/v1/mounts/" + Uri.EscapeDataString(id) + "/remount", content: null, ct);
        if (!resp.IsSuccessStatusCode)
            throw new ApiException(await ReadError(resp, ct), (int)resp.StatusCode);
        return await resp.Content.ReadFromJsonAsync<MountView>(JsonOpts, ct);
    }

    public void Dispose() => _http.Dispose();
}

public sealed class ApiException : Exception
{
    public int StatusCode { get; }
    public ApiException(string message, int statusCode) : base(message) => StatusCode = statusCode;
}
