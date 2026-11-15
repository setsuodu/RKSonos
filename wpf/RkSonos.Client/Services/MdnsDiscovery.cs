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
    /// <summary>界面展示：优先友好名，否则只显示地址</summary>
    public string Display => string.IsNullOrWhiteSpace(Name) || Name == "RKSonos"
        ? Address
        : $"{Name}  ·  {Host}";
}

/// <summary>
/// 发现 RKSonos：上次地址 → 并行(mDNS 短超时 + 网段 :8080/health)。
/// Windows 上 mDNS 常失效，HTTP 扫描是主路径；结果边发现边进列表。
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
        var found = new Dictionary<string, DiscoveredDevice>(StringComparer.OrdinalIgnoreCase);
        var gate = new object();

        void Publish(DiscoveredDevice d)
        {
            var key = $"{d.Host}:{d.Port}";
            lock (gate)
            {
                if (found.ContainsKey(key)) return;
                found[key] = d;
            }

            var app = System.Windows.Application.Current;
            if (app == null) return;
            _ = app.Dispatcher.InvokeAsync(() =>
            {
                if (target.Any(x =>
                        string.Equals(x.Host, d.Host, StringComparison.OrdinalIgnoreCase) &&
                        x.Port == d.Port))
                    return;
                target.Add(d);
            });
        }

        await System.Windows.Application.Current.Dispatcher.InvokeAsync(() => target.Clear());

        // 1) 上次 / 手动地址：最快路径
        progress?.Report("正在搜索…");
        foreach (var url in hintUrls ?? Array.Empty<string>())
        {
            ct.ThrowIfCancellationRequested();
            var d = await TryHealthAsync(url, "最近", ct).ConfigureAwait(false);
            if (d != null) Publish(d);
        }

        // 2) mDNS + 网段扫描并行；总预算约 3.5s
        var mdnsTask = BrowseMdnsAsync(Publish, ct);
        var lanTask = ProbeLanAsync(Publish, found, ct);

        var all = Task.WhenAll(mdnsTask, lanTask);
        await Task.WhenAny(all, Task.Delay(TimeSpan.FromSeconds(3.5), ct)).ConfigureAwait(false);

        // 给已在飞的请求一点点收尾时间
        try { await Task.WhenAny(all, Task.Delay(400, ct)).ConfigureAwait(false); }
        catch { /* ignore */ }

        progress?.Report(found.Count > 0 ? $"找到 {found.Count} 台" : "未找到音箱");
    }

    private static async Task BrowseMdnsAsync(Action<DiscoveredDevice> publish, CancellationToken ct)
    {
        try
        {
            // 短超时、少重试：Windows 上经常整段白等
            var results = await ZeroconfResolver.ResolveAsync(
                "_rksonos._tcp.local.",
                scanTime: TimeSpan.FromSeconds(1.2),
                retries: 1,
                retryDelayMilliseconds: 200).ConfigureAwait(false);

            foreach (var host in results)
            {
                ct.ThrowIfCancellationRequested();
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
                    // mDNS 命中后仍用 /health 确认，避免脏条目
                    var d = await TryHealthAsync($"http://{ip}:{port}", name, ct).ConfigureAwait(false);
                    if (d != null)
                    {
                        publish(new DiscoveredDevice
                        {
                            Name = name,
                            Host = d.Host,
                            Port = d.Port,
                            Source = "mDNS",
                        });
                    }
                }
            }
        }
        catch
        {
            // 无组播 / 防火墙：忽略
        }
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

            using var http = new HttpClient { Timeout = TimeSpan.FromMilliseconds(800) };
            using var resp = await http.GetAsync($"{raw}/health", ct).ConfigureAwait(false);
            if (!resp.IsSuccessStatusCode) return null;
            var body = (await resp.Content.ReadAsStringAsync(ct).ConfigureAwait(false)).Trim();
            if (!body.Equals("ok", StringComparison.OrdinalIgnoreCase)) return null;

            return new DiscoveredDevice
            {
                Name = source is "最近" or "候选" or "网段" ? "RKSonos" : source,
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

    private static async Task ProbeLanAsync(
        Action<DiscoveredDevice> publish,
        Dictionary<string, DiscoveredDevice> found,
        CancellationToken ct)
    {
        var prefixes = GetLocalIpv4Prefixes();
        if (prefixes.Count == 0) return;

        // 只扫本机所在的 1 个主网段，避免多网卡拖时间
        var prefix = prefixes[0];
        var sem = new SemaphoreSlim(64);
        var tasks = new List<Task>();

        for (var i = 1; i <= 254; i++)
        {
            ct.ThrowIfCancellationRequested();
            var ip = $"{prefix}.{i}";
            var key = $"{ip}:8080";
            lock (found)
            {
                if (found.ContainsKey(key)) continue;
            }

            tasks.Add(Task.Run(async () =>
            {
                await sem.WaitAsync(ct).ConfigureAwait(false);
                try
                {
                    // 先 TCP 探活（比 HTTP 超时快得多）
                    if (!await TcpOpenAsync(ip, 8080, 180, ct).ConfigureAwait(false))
                        return;

                    var d = await TryHealthAsync($"http://{ip}:8080", "网段", ct).ConfigureAwait(false);
                    if (d != null) publish(d);
                }
                catch { /* ignore */ }
                finally { sem.Release(); }
            }, ct));
        }

        var all = Task.WhenAll(tasks);
        await Task.WhenAny(all, Task.Delay(TimeSpan.FromSeconds(3), ct)).ConfigureAwait(false);
    }

    private static async Task<bool> TcpOpenAsync(string host, int port, int timeoutMs, CancellationToken ct)
    {
        try
        {
            using var tcp = new TcpClient();
            using var reg = ct.Register(() => { try { tcp.Close(); } catch { } });
            var connect = tcp.ConnectAsync(host, port);
            var done = await Task.WhenAny(connect, Task.Delay(timeoutMs, ct)).ConfigureAwait(false);
            if (done != connect) return false;
            await connect.ConfigureAwait(false);
            return tcp.Connected;
        }
        catch
        {
            return false;
        }
    }

    private static List<string> GetLocalIpv4Prefixes()
    {
        var list = new List<string>();
        foreach (var ni in NetworkInterface.GetAllNetworkInterfaces())
        {
            if (ni.OperationalStatus != OperationalStatus.Up) continue;
            if (ni.NetworkInterfaceType is NetworkInterfaceType.Loopback) continue;
            // 优先 Wi-Fi / 以太网，跳过虚拟网卡常见前缀在后面去重即可
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
