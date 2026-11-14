using System.Collections.ObjectModel;
using System.Net.Http;
using System.Net.NetworkInformation;
using System.Net.Sockets;
using Zeroconf;

namespace RkSonos.Client.Services;

public sealed class DiscoveredDevice
{
    public string Name { get; init; } = "";
    public string Host { get; init; } = "";
    public int Port { get; init; } = 8080;
    public string Source { get; init; } = "";
    public string Address => $"http://{Host}:{Port}";
    public string Display => string.IsNullOrWhiteSpace(Name)
        ? $"{Address} ({Source})"
        : $"{Name}  ·  {Address} ({Source})";
}

/// <summary>
/// 发现 RKSonos：候选 URL → mDNS → 本机网段 /health 扫描。
/// Windows 上 mDNS 常被防火墙/交换机丢掉，HTTP 兜底是必要路径。
/// </summary>
public static class MdnsDiscovery
{
    public static async Task BrowseAsync(
        ObservableCollection<DiscoveredDevice> target,
        CancellationToken ct,
        IEnumerable<string>? hintUrls = null,
        TimeSpan? browseTimeout = null,
        IProgress<string>? progress = null)
    {
        var timeout = browseTimeout ?? TimeSpan.FromSeconds(3);
        var found = new Dictionary<string, DiscoveredDevice>(StringComparer.OrdinalIgnoreCase);

        void Add(DiscoveredDevice d)
        {
            var key = $"{d.Host}:{d.Port}";
            if (found.ContainsKey(key)) return;
            found[key] = d;
        }

        progress?.Report("探测候选地址…");
        foreach (var url in hintUrls ?? Array.Empty<string>())
        {
            ct.ThrowIfCancellationRequested();
            var d = await TryHealthAsync(url, "候选", ct).ConfigureAwait(false);
            if (d != null) Add(d);
        }

        progress?.Report("mDNS 浏览 _rksonos._tcp…");
        foreach (var st in new[] { "_rksonos._tcp.local.", "_rksonos._tcp" })
        {
            ct.ThrowIfCancellationRequested();
            try
            {
                var results = await ZeroconfResolver.ResolveAsync(
                    st,
                    scanTime: timeout,
                    retries: 2,
                    retryDelayMilliseconds: 300).ConfigureAwait(false);

                foreach (var host in results)
                {
                    if (host?.IPAddresses == null || host.IPAddresses.Count == 0)
                        continue;

                    var port = 8080;
                    if (host.Services != null)
                    {
                        foreach (var svc in host.Services.Values)
                        {
                            if (svc.Port > 0)
                            {
                                port = svc.Port;
                                break;
                            }
                        }
                    }

                    var name = string.IsNullOrWhiteSpace(host.DisplayName) ? "RKSonos" : host.DisplayName;
                    foreach (var ip in host.IPAddresses)
                    {
                        if (string.IsNullOrWhiteSpace(ip) || ip.Contains(':'))
                            continue;
                        Add(new DiscoveredDevice
                        {
                            Name = name,
                            Host = ip,
                            Port = port,
                            Source = "mDNS",
                        });
                    }
                }
            }
            catch
            {
                // Windows 无组播时很常见
            }
        }

        progress?.Report("扫描局域网 :8080/health…");
        try
        {
            await ProbeLanAsync(found, ct).ConfigureAwait(false);
        }
        catch { /* ignore */ }

        await System.Windows.Application.Current.Dispatcher.InvokeAsync(() =>
        {
            target.Clear();
            foreach (var d in found.Values.OrderBy(x => x.Name).ThenBy(x => x.Host))
                target.Add(d);
        });
    }

    public static async Task<DiscoveredDevice?> TryHealthAsync(string urlOrHost, string source, CancellationToken ct)
    {
        try
        {
            var raw = (urlOrHost ?? "").Trim();
            if (string.IsNullOrWhiteSpace(raw)) return null;
            if (!raw.StartsWith("http://", StringComparison.OrdinalIgnoreCase) &&
                !raw.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
                raw = "http://" + raw;
            raw = raw.TrimEnd('/');

            if (!Uri.TryCreate(raw, UriKind.Absolute, out var uri))
                return null;
            var host = uri.Host;
            var port = uri.IsDefaultPort ? 8080 : uri.Port;

            using var http = new HttpClient { Timeout = TimeSpan.FromSeconds(2) };
            using var resp = await http.GetAsync($"{raw}/health", ct).ConfigureAwait(false);
            if (!resp.IsSuccessStatusCode) return null;
            var body = (await resp.Content.ReadAsStringAsync(ct).ConfigureAwait(false)).Trim();
            if (!body.Equals("ok", StringComparison.OrdinalIgnoreCase)) return null;

            return new DiscoveredDevice
            {
                Name = "RKSonos",
                Host = host,
                Port = port,
                Source = source,
            };
        }
        catch
        {
            return null;
        }
    }

    private static async Task ProbeLanAsync(Dictionary<string, DiscoveredDevice> found, CancellationToken ct)
    {
        var prefixes = GetLocalIpv4Prefixes();
        if (prefixes.Count == 0) return;

        using var http = new HttpClient { Timeout = TimeSpan.FromMilliseconds(600) };
        var gate = new SemaphoreSlim(48);
        var tasks = new List<Task>();

        foreach (var prefix in prefixes.Take(3))
        {
            for (var i = 1; i <= 254; i++)
            {
                ct.ThrowIfCancellationRequested();
                var ip = $"{prefix}.{i}";
                var key = $"{ip}:8080";
                if (found.ContainsKey(key)) continue;

                tasks.Add(Task.Run(async () =>
                {
                    await gate.WaitAsync(ct).ConfigureAwait(false);
                    try
                    {
                        using var resp = await http.GetAsync($"http://{ip}:8080/health", ct)
                            .ConfigureAwait(false);
                        if (!resp.IsSuccessStatusCode) return;
                        var body = (await resp.Content.ReadAsStringAsync(ct).ConfigureAwait(false)).Trim();
                        if (!body.Equals("ok", StringComparison.OrdinalIgnoreCase)) return;
                        lock (found)
                        {
                            if (!found.ContainsKey(key))
                            {
                                found[key] = new DiscoveredDevice
                                {
                                    Name = "RKSonos",
                                    Host = ip,
                                    Port = 8080,
                                    Source = "网段扫描",
                                };
                            }
                        }
                    }
                    catch { }
                    finally { gate.Release(); }
                }, ct));
            }
        }

        var all = Task.WhenAll(tasks);
        await Task.WhenAny(all, Task.Delay(TimeSpan.FromSeconds(12), ct)).ConfigureAwait(false);
    }

    private static List<string> GetLocalIpv4Prefixes()
    {
        var list = new List<string>();
        foreach (var ni in NetworkInterface.GetAllNetworkInterfaces())
        {
            if (ni.OperationalStatus != OperationalStatus.Up) continue;
            if (ni.NetworkInterfaceType is NetworkInterfaceType.Loopback) continue;
            foreach (var ua in ni.GetIPProperties().UnicastAddresses)
            {
                if (ua.Address.AddressFamily != AddressFamily.InterNetwork) continue;
                var s = ua.Address.ToString();
                var parts = s.Split('.');
                if (parts.Length != 4) continue;
                if (s.StartsWith("127.") || s.StartsWith("169.254.")) continue;
                list.Add($"{parts[0]}.{parts[1]}.{parts[2]}");
            }
        }
        return list.Distinct().ToList();
    }
}
