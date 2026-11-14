using System.IO;
using System.Net.Http;

namespace RkSonos.Client.Services;

/// <summary>记住上次成功连上的 server 地址（%LocalAppData%\RKSonos\last_server.txt）。</summary>
public static class LastServerStore
{
    public static string FilePath { get; } = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "RKSonos", "last_server.txt");

    public static string? Load()
    {
        try
        {
            if (!File.Exists(FilePath)) return null;
            var s = File.ReadAllText(FilePath).Trim();
            return string.IsNullOrWhiteSpace(s) ? null : s;
        }
        catch
        {
            return null;
        }
    }

    public static void Save(string url)
    {
        try
        {
            var dir = Path.GetDirectoryName(FilePath);
            if (!string.IsNullOrEmpty(dir))
                Directory.CreateDirectory(dir);
            File.WriteAllText(FilePath, url.Trim());
        }
        catch
        {
            // ignore
        }
    }

    public static void Clear()
    {
        try
        {
            if (File.Exists(FilePath))
                File.Delete(FilePath);
        }
        catch { }
    }

    public static async Task<string?> TryReachAsync(string? url, TimeSpan? timeout = null)
    {
        if (string.IsNullOrWhiteSpace(url)) return null;
        var baseUrl = url.Trim().TrimEnd('/');
        if (!baseUrl.StartsWith("http://", StringComparison.OrdinalIgnoreCase) &&
            !baseUrl.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
            baseUrl = "http://" + baseUrl;

        try
        {
            using var http = new HttpClient { Timeout = timeout ?? TimeSpan.FromSeconds(2) };
            var body = (await http.GetStringAsync(baseUrl + "/health").ConfigureAwait(false)).Trim();
            if (!body.Equals("ok", StringComparison.OrdinalIgnoreCase))
                return null;
            return baseUrl;
        }
        catch
        {
            return null;
        }
    }
}
