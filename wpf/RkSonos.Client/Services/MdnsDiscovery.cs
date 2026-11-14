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
    public string Address => $"http://{Host}:{Port}";
    public string Display => string.IsNullOrWhiteSpace(Name)
        ? Address
        : $"{Name}  ·  {Address}";
}

/// <summary>
/// Browse <c>_rksonos._tcp.local.</c> via mDNS, with LAN HTTP /health probe fallback.
/// </summary>
public static class MdnsDiscovery
{
    public const string ServiceType = "_rksonos._tcp.local.";

    public static async Task BrowseAsync(
        ObservableCollection<DiscoveredDevice> target,
        CancellationToken ct,
        TimeSpan? browseTimeout = null)
    {
        var timeout = browseTimeout ?? TimeSpan.FromSeconds(4);
        var found = new Dictionary<string, DiscoveredDevice>(StringComparer.OrdinalIgnoreCase);

        try
        {
            // Zeroconf: protocol without trailing .local sometimes; try both
            var results = await ZeroconfResolver.ResolveAsync(
                ServiceType,
                scanTime: timeout,
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

                foreach (var ip in host.IPAddresses)
                {
                    if (string.IsNullOrWhiteSpace(ip) || ip.Contains(':')) // skip v6 for simplicity
                        continue;
                    var key = $"{ip}:{port}";
                    if (found.ContainsKey(key)) continue;
                    found[key] = new DiscoveredDevice
                    {
                        Name = string.IsNullOrWhiteSpace(host.DisplayName) ? "RKSonos" : host.DisplayName,
                        Host = ip,
                        Port = port,
                    };
                }
            }
        }
        catch
        {
            // mDNS may fail on some NICs / VPN; fall through to probe
        }

        // Fallback: probe common LAN gateways' /24 for :8080 /health == ok
        try
        {
            await ProbeLanAsync(found, ct).ConfigureAwait(false);
        }
        catch
        {
            // ignore
        }

        // Marshal to UI collection
        await System.Windows.Application.Current.Dispatcher.InvokeAsync(() =>
        {
            foreach (var d in found.Values.OrderBy(x => x.Name).ThenBy(x => x.Host))
            {
                if (target.Any(t => t.Address.Equals(d.Address, StringComparison.OrdinalIgnoreCase)))
                    continue;
                target.Add(d);
            }
        });
    }

    private static async Task ProbeLanAsync(Dictionary<string, DiscoveredDevice> found, CancellationToken ct)
    {
        var prefixes = GetLocalIpv4Prefixes();
        if (prefixes.Count == 0) return;

        using var http = new HttpClient { Timeout = TimeSpan.FromMilliseconds(350) };
        var tasks = new List<Task>();
        var gate = new SemaphoreSlim(32);

        foreach (var prefix in prefixes.Take(2)) // at most 2 subnets
        {
            for (int i = 1; i <= 254; i++)
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
                            found[key] = new DiscoveredDevice
                            {
                                Name = "RKSonos",
                                Host = ip,
                                Port = 8080,
                            };
                        }
                    }
                    catch
                    {
                        // timeout / refuse
                    }
                    finally
                    {
                        gate.Release();
                    }
                }, ct));
            }
        }

        // Don't wait forever
        var all = Task.WhenAll(tasks);
        var done = await Task.WhenAny(all, Task.Delay(TimeSpan.FromSeconds(6), ct)).ConfigureAwait(false);
        _ = done;
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
                if (s.StartsWith("127.")) continue;
                list.Add($"{parts[0]}.{parts[1]}.{parts[2]}");
            }
        }
        return list.Distinct().ToList();
    }
}
