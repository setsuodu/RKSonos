using System.Collections.ObjectModel;
using System.Net.WebSockets;
using System.Text;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Threading;
using RkSonos.Client.Models;
using RkSonos.Client.Services;

namespace RkSonos.Client;

public partial class MainWindow : Window
{
    private readonly RkSonosApi _api = new();
    private readonly DispatcherTimer _pollTimer;
    private readonly ObservableCollection<BrowseItem> _browse = new();
    private readonly ObservableCollection<QueueItem> _queue = new();
    private List<Track> _allTracks = new();
    private List<MountView> _mounts = new();
    private readonly List<string> _navStack = new();

    private bool _connected;
    private bool _suppressVolume;
    private bool _suppressMode;
    private bool _draggingVolume;
    private bool _draggingProgress;
    private bool _busy;
    private string _lastState = "idle";
    private string? _serverVersion;
    private string _serverUrl = "http://127.0.0.1:8080";
    private string _connDetail = "未连接";
    private ClientWebSocket? _ws;
    private CancellationTokenSource? _wsCts;

    public string ServerUrl => _serverUrl;
    public bool IsConnected => _connected;
    public string ConnectionDetail => _connDetail;

    public MainWindow() : this("http://127.0.0.1:8080") { }

    public MainWindow(string serverBaseUrl)
    {
        InitializeComponent();
        ListLibrary.ItemsSource = _browse;
        ListQueue.ItemsSource = _queue;
        _serverUrl = string.IsNullOrWhiteSpace(serverBaseUrl) ? "http://127.0.0.1:8080" : serverBaseUrl.Trim();

        _pollTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(2) };
        _pollTimer.Tick += async (_, _) =>
        {
            if (_connected && !_busy)
                await SafeRefreshStatusAsync();
        };

        Loaded += async (_, _) => await ConnectAsync();
        Closed += (_, _) =>
        {
            _pollTimer.Stop();
            StopWs();
            _api.Dispose();
        };
    }

    private void SetStatus(string msg) => TxtStatus.Text = msg;

    private void SetConnected(bool ok, string detail)
    {
        _connected = ok;
        _connDetail = detail;
        DotStatus.Fill = ok
            ? (Brush)FindResource("OkBrush")
            : (Brush)FindResource("DangerBrush");
        TxtConn.Text = detail;
        if (ok)
        {
            _pollTimer.Start();
            _ = StartWsAsync();
        }
        else
        {
            _pollTimer.Stop();
            StopWs();
        }
    }

    private async Task ConnectAsync()
    {
        if (_busy) return;
        _busy = true;
        var ok = false;
        try
        {
            _api.BaseUrl = _serverUrl;
            SetStatus($"连接 {_api.BaseUrl} ...");
            var health = await _api.HealthAsync();
            if (!string.Equals(health, "ok", StringComparison.OrdinalIgnoreCase))
                throw new Exception($"health={health}");

            SetConnected(true, "已连接");
            SetStatus($"已连接 {_api.BaseUrl}");
            ok = true;
        }
        catch (Exception ex)
        {
            SetConnected(false, "连接失败");
            SetStatus("连接失败: " + ex.Message);
        }
        finally
        {
            _busy = false;
        }

        // 必须在释放 _busy 之后刷新，否则 RefreshAllAsync 会直接 return
        if (ok)
            await RefreshAllAsync();
    }

    public async Task ConnectFromSettingsAsync(string url)
    {
        if (!string.IsNullOrWhiteSpace(url))
            _serverUrl = url.Trim();
        await ConnectAsync();
    }

    public async Task RefreshFromSettingsAsync()
    {
        if (!_connected)
        {
            await ConnectAsync();
            return;
        }
        await RefreshAllAsync();
    }

    public void SwitchDevice()
    {
        // 必须先把 Application.MainWindow 切走，再 Close，
        // 否则 OnMainWindowClose 会直接杀进程（看起来像 crash）
        var disc = new DiscoveryWindow();
        Application.Current.MainWindow = disc;
        disc.Show();
        Close();
    }

    private async Task RefreshAllAsync()
    {
        if (_busy) return;
        _busy = true;
        try
        {
            await SafeRefreshStatusAsync();
            await LoadLibraryAsync();
            await LoadQueueAsync();
        }
        finally
        {
            _busy = false;
        }
    }

    private async Task SafeRefreshStatusAsync()
    {
        try
        {
            var st = await _api.GetStatusAsync();
            if (st != null)
                ApplyStatus(st);
        }
        catch { /* ignore poll errors */ }
    }

    private void ApplyStatus(StatusResponse st)
    {
        _lastState = st.State ?? "idle";
        if (!string.IsNullOrWhiteSpace(st.ServerVersion))
            _serverVersion = st.ServerVersion;
        var name = string.IsNullOrWhiteSpace(st.Path) ? "—" : System.IO.Path.GetFileName(st.Path);
        TxtNowPlaying.Text = name;
        TxtMeta.Text =
            $"状态: {st.State}  ·  模式: {st.Mode}  ·  {st.Path}";

        // 播放列表：高亮当前 + 「第 N / M 首」
        var qi = st.QueueIndex;
        for (int i = 0; i < _queue.Count; i++)
            _queue[i].IsCurrent = qi == i;
        if (_queue.Count == 0)
            TxtQueueCount.Text = "— / 0";
        else if (qi is int idx && idx >= 0)
            TxtQueueCount.Text = $"第 {idx + 1} / {_queue.Count} 首";
        else
            TxtQueueCount.Text = $"— / {_queue.Count} 首";

        BtnPlayPause.Content = string.Equals(st.State, "playing", StringComparison.OrdinalIgnoreCase)
            ? "\uE769"   // Pause
            : "\uE768";  // Play

        if (!_draggingVolume)
        {
            _suppressVolume = true;
            SliderVolume.Value = Math.Clamp(st.Volume, 0, 100);
            TxtVolume.Text = ((int)SliderVolume.Value).ToString();
            _suppressVolume = false;
        }

        _suppressMode = true;
        SelectMode(st.Mode);
        _suppressMode = false;

        UpdateProgress(st.Position, st.Duration);
    }

    private static string FmtTime(double sec)
    {
        if (double.IsNaN(sec) || sec < 0) sec = 0;
        var ts = TimeSpan.FromSeconds(sec);
        return ts.TotalHours >= 1
            ? $"{(int)ts.TotalHours}:{ts.Minutes:D2}:{ts.Seconds:D2}"
            : $"{ts.Minutes}:{ts.Seconds:D2}";
    }

    private void UpdateProgress(double? position, double? duration)
    {
        if (_draggingProgress) return;
        var pos = position ?? 0;
        var dur = duration ?? 0;
        TxtPos.Text = FmtTime(pos);
        TxtDur.Text = dur > 0 ? FmtTime(dur) : "0:00";
        if (dur > 0.5)
        {
            SliderProgress.Maximum = dur;
            SliderProgress.Value = Math.Clamp(pos, 0, dur);
            SliderProgress.IsEnabled = true;
        }
        else
        {
            SliderProgress.Maximum = 100;
            SliderProgress.Value = 0;
            SliderProgress.IsEnabled = false;
        }
    }

    private async Task StartWsAsync()
    {
        StopWs();
        _wsCts = new CancellationTokenSource();
        var ct = _wsCts.Token;
        var ws = new ClientWebSocket();
        _ws = ws;
        try
        {
            var baseUrl = _api.BaseUrl.TrimEnd('/');
            if (baseUrl.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
                baseUrl = "wss://" + baseUrl["https://".Length..];
            else if (baseUrl.StartsWith("http://", StringComparison.OrdinalIgnoreCase))
                baseUrl = "ws://" + baseUrl["http://".Length..];
            else
                baseUrl = "ws://" + baseUrl;

            await ws.ConnectAsync(new Uri(baseUrl + "/ws"), ct);
            SetStatus("已连接 (WS)");
            var buf = new byte[8192];
            while (!ct.IsCancellationRequested && ws.State == WebSocketState.Open)
            {
                using var ms = new System.IO.MemoryStream();
                WebSocketReceiveResult result;
                do
                {
                    result = await ws.ReceiveAsync(buf, ct);
                    if (result.MessageType == WebSocketMessageType.Close)
                    {
                        await ws.CloseAsync(WebSocketCloseStatus.NormalClosure, "", CancellationToken.None);
                        return;
                    }
                    ms.Write(buf, 0, result.Count);
                } while (!result.EndOfMessage);

                if (result.MessageType != WebSocketMessageType.Text) continue;
                var json = Encoding.UTF8.GetString(ms.ToArray());
                StatusResponse? st = null;
                try
                {
                    st = JsonSerializer.Deserialize<StatusResponse>(json,
                        new JsonSerializerOptions { PropertyNameCaseInsensitive = true });
                }
                catch { /* ignore */ }
                if (st == null) continue;
                await Dispatcher.InvokeAsync(() => ApplyStatus(st));
            }
        }
        catch (OperationCanceledException) { }
        catch (Exception ex)
        {
            await Dispatcher.InvokeAsync(() => SetStatus("WS 断开: " + ex.Message));
        }
        finally
        {
            try { ws.Dispose(); } catch { }
            if (ReferenceEquals(_ws, ws)) _ws = null;
        }
    }

    private void StopWs()
    {
        try { _wsCts?.Cancel(); } catch { }
        _wsCts = null;
        try
        {
            if (_ws != null && _ws.State == WebSocketState.Open)
                _ = _ws.CloseAsync(WebSocketCloseStatus.NormalClosure, "", CancellationToken.None);
        }
        catch { }
        try { _ws?.Dispose(); } catch { }
        _ws = null;
    }

    private string _currentMode = "normal";

    private void SelectMode(string mode)
    {
        mode = (mode ?? "normal").ToLowerInvariant();
        _currentMode = mode;
        // Segoe MDL2: Repeat=E8EE, RepeatOne≈E8ED, Shuffle=E8B1
        switch (mode)
        {
            case "repeat_one":
                BtnMode.Content = "\uE8ED";
                BtnMode.ToolTip = "播放模式: 单曲循环";
                BtnMode.Opacity = 1.0;
                break;
            case "repeat_all":
                BtnMode.Content = "\uE8EE";
                BtnMode.ToolTip = "播放模式: 列表循环";
                BtnMode.Opacity = 1.0;
                break;
            case "shuffle":
                BtnMode.Content = "\uE8B1";
                BtnMode.ToolTip = "播放模式: 随机";
                BtnMode.Opacity = 1.0;
                break;
            default:
                BtnMode.Content = "\uE8EE";
                BtnMode.ToolTip = "播放模式: 顺序播放";
                BtnMode.Opacity = 0.45;
                break;
        }
    }

    private async void BtnMode_Click(object sender, RoutedEventArgs e)
    {
        if (!_connected || _suppressMode) return;
        var next = _currentMode switch
        {
            "normal" => "repeat_all",
            "repeat_all" => "repeat_one",
            "repeat_one" => "shuffle",
            _ => "normal",
        };
        await RunCmd(() => _api.SetModeAsync(next));
    }

    // ----- 音乐库层级浏览 -----

    private async Task LoadLibraryAsync(string? q = null)
    {
        // 根目录：音乐库设置里的挂载
        try
        {
            var mounts = await _api.GetMountsAsync();
            _mounts = mounts?.Mounts ?? new List<MountView>();
        }
        catch (Exception ex)
        {
            _mounts = new List<MountView>();
            SetStatus("读取挂载失败: " + ex.Message);
        }

        if (_navStack.Count == 0)
            RenderRoot();

        // 曲目索引（服务端 library.json / scan 结果）
        try
        {
            var libSt = await _api.GetLibraryStatusAsync();
            var tracks = new List<Track>();
            for (int offset = 0; offset < 20000; offset += 1000)
            {
                var batch = await _api.GetLibraryAsync(null, limit: 1000, offset: offset);
                if (batch.Count == 0) break;
                tracks.AddRange(batch);
                if (batch.Count < 1000) break;
            }

            // 有挂载但还没索引：自动扫一次（不在界面放扫库按钮）
            if (tracks.Count == 0 && _mounts.Count > 0 && (libSt == null || !libSt.Scanning))
            {
                try
                {
                    TxtLibInfo.Text = "正在索引…";
                    var r = await _api.LibraryScanAsync();
                    tracks.Clear();
                    for (int offset = 0; offset < 20000; offset += 1000)
                    {
                        var batch = await _api.GetLibraryAsync(null, limit: 1000, offset: offset);
                        if (batch.Count == 0) break;
                        tracks.AddRange(batch);
                        if (batch.Count < 1000) break;
                    }
                    SetStatus(r == null ? "索引完成" : $"索引完成: {r.Count} 首");
                }
                catch (Exception ex)
                {
                    SetStatus("自动索引失败: " + ex.Message);
                }
            }

            _allTracks = tracks;
            TxtLibInfo.Text = $"{_mounts.Count} 个源 · {_allTracks.Count} 首";

            if (_navStack.Count == 0)
                RenderRoot();
            else
                RenderNav(_navStack[^1]);
        }
        catch (Exception ex)
        {
            if (_navStack.Count == 0)
                RenderRoot();
            SetStatus("读取索引失败: " + ex.Message);
        }
    }

    private void RenderRoot()
    {
        _browse.Clear();
        BtnLibBack.IsEnabled = false;
        TxtLibPath.Text = "文件夹";
        foreach (var m in _mounts)
        {
            var title = m.Uri;
            if (title.StartsWith("smb://", StringComparison.OrdinalIgnoreCase))
                title = "//" + title["smb://".Length..];
            if (string.IsNullOrWhiteSpace(title))
                title = m.Label;
            _browse.Add(new BrowseItem
            {
                Kind = BrowseKind.Mount,
                Title = title,
                Subtitle = "",
                NavKey = m.Id,
            });
        }
        if (_browse.Count == 0)
            TxtLibPath.Text = "文件夹（请先在 设置 → 音乐库设置 添加）";
    }

    private void RenderNav(string navKey)
    {
        _browse.Clear();
        BtnLibBack.IsEnabled = _navStack.Count > 0;
        TxtLibPath.Text = string.IsNullOrEmpty(navKey) ? "文件夹" : "//" + navKey.Replace('\\', '/');

        var prefix = navKey.Trim().TrimEnd('/');
        var folders = new SortedSet<string>(StringComparer.OrdinalIgnoreCase);
        var files = new List<Track>();

        foreach (var tr in _allTracks)
        {
            var rel = (tr.RelPath ?? "").Replace('\\', '/').Trim('/');
            if (string.IsNullOrEmpty(rel)) continue;

            string rest;
            if (rel.StartsWith(prefix + "/", StringComparison.OrdinalIgnoreCase))
            {
                rest = rel[(prefix.Length + 1)..];
            }
            else if (tr.MountId != null &&
                     string.Equals(tr.MountId, prefix, StringComparison.OrdinalIgnoreCase))
            {
                // rel 可能已含 mountId 前缀，或仅为相对路径
                if (rel.StartsWith(prefix + "/", StringComparison.OrdinalIgnoreCase))
                    rest = rel[(prefix.Length + 1)..];
                else if (rel.Equals(prefix, StringComparison.OrdinalIgnoreCase))
                    continue;
                else
                    rest = rel;
            }
            else
                continue;

            if (string.IsNullOrEmpty(rest)) continue;
            var slash = rest.IndexOf('/');
            if (slash < 0)
                files.Add(tr);
            else
                folders.Add(rest[..slash]);
        }

        foreach (var folder in folders)
        {
            _browse.Add(new BrowseItem
            {
                Kind = BrowseKind.Folder,
                Title = folder,
                Subtitle = "",
                NavKey = prefix + "/" + folder,
            });
        }

        foreach (var tr in files.OrderBy(x => x.Name, StringComparer.OrdinalIgnoreCase))
        {
            _browse.Add(new BrowseItem
            {
                Kind = BrowseKind.Track,
                Title = string.IsNullOrWhiteSpace(tr.Name) ? System.IO.Path.GetFileName(tr.Path) : tr.Name,
                Subtitle = "",
                NavKey = prefix,
                TrackPath = tr.Path,
                Track = tr,
            });
        }
    }


    private void EnterItem(BrowseItem item)
    {
        if (item.Kind == BrowseKind.Track) return;
        _navStack.Add(item.NavKey);
        RenderNav(item.NavKey);
    }

    private void BtnLibBack_Click(object sender, RoutedEventArgs e)
    {
        if (_navStack.Count == 0)
        {
            RenderRoot();
            return;
        }
        _navStack.RemoveAt(_navStack.Count - 1);
        if (_navStack.Count == 0)
            RenderRoot();
        else
            RenderNav(_navStack[^1]);
    }

    private BrowseItem? SelectedBrowse() => ListLibrary.SelectedItem as BrowseItem;

    private async Task PlayBrowseItem(BrowseItem item)
    {
        // 播放 = 替换整份播放列表（不是追加）
        List<string> paths;
        int start = 0;

        if (item.Kind == BrowseKind.Track && !string.IsNullOrEmpty(item.TrackPath))
        {
            // 同层级全部音频，从点中的这首开始
            var parent = ParentPrefixOfTrack(item);
            paths = TracksUnderPrefix(parent, immediateOnly: true)
                .Select(tr => tr.Path)
                .ToList();
            if (paths.Count == 0)
                paths = new List<string> { item.TrackPath! };
            start = paths.FindIndex(p => string.Equals(p, item.TrackPath, StringComparison.OrdinalIgnoreCase));
            if (start < 0) start = 0;
        }
        else
        {
            // 文件夹 / 源：该节点下全部音频（含子文件夹）
            var prefix = item.NavKey.Trim().TrimEnd('/');
            paths = TracksUnderPrefix(prefix, immediateOnly: false)
                .Select(tr => tr.Path)
                .ToList();
        }

        if (paths.Count == 0)
        {
            SetStatus("没有可播放的音频");
            return;
        }

        await RunCmd(() => _api.SetQueueAsync(paths, start));
        await LoadQueueAsync();
    }

    /// <summary>曲目所在目录的 NavKey（mountId 或 mountId/folder/...）</summary>
    private string ParentPrefixOfTrack(BrowseItem item)
    {
        if (!string.IsNullOrEmpty(item.NavKey))
            return item.NavKey.Trim().TrimEnd('/');
        var rel = (item.Track?.RelPath ?? "").Replace('\\', '/').Trim('/');
        var idx = rel.LastIndexOf('/');
        return idx <= 0 ? rel : rel[..idx];
    }

    private IEnumerable<Track> TracksUnderPrefix(string prefix, bool immediateOnly)
    {
        prefix = (prefix ?? "").Trim().TrimEnd('/');
        foreach (var tr in _allTracks)
        {
            var rel = (tr.RelPath ?? "").Replace('\\', '/').Trim('/');
            string rest;
            if (!string.IsNullOrEmpty(prefix) &&
                rel.StartsWith(prefix + "/", StringComparison.OrdinalIgnoreCase))
            {
                rest = rel[(prefix.Length + 1)..];
            }
            else if (!string.IsNullOrEmpty(prefix) &&
                     tr.MountId != null &&
                     string.Equals(tr.MountId, prefix, StringComparison.OrdinalIgnoreCase))
            {
                if (rel.StartsWith(prefix + "/", StringComparison.OrdinalIgnoreCase))
                    rest = rel[(prefix.Length + 1)..];
                else if (rel.Equals(prefix, StringComparison.OrdinalIgnoreCase))
                    continue;
                else
                    rest = rel;
            }
            else if (string.IsNullOrEmpty(prefix))
            {
                rest = rel;
            }
            else
                continue;

            if (string.IsNullOrEmpty(rest)) continue;
            if (immediateOnly)
            {
                if (rest.Contains('/')) continue; // 只要本层文件
            }
            yield return tr;
        }
    }

    private async Task AddBrowseToQueue(BrowseItem item)
    {
        if (item.Kind == BrowseKind.Track && !string.IsNullOrEmpty(item.TrackPath))
        {
            await RunCmd(() => _api.QueueAddAsync(new[] { item.TrackPath! }));
            await LoadQueueAsync();
            return;
        }

        var prefix = item.NavKey.Trim().TrimEnd('/');
        var paths = _allTracks
            .Where(tr =>
            {
                var rel = (tr.RelPath ?? "").Replace('\\', '/').Trim('/');
                return rel.StartsWith(prefix + "/", StringComparison.OrdinalIgnoreCase)
                       || (tr.MountId != null && prefix == tr.MountId);
            })
            .Select(tr => tr.Path)
            .ToList();

        if (paths.Count == 0)
        {
            SetStatus("没有可添加的歌曲");
            return;
        }
        await RunCmd(() => _api.QueueAddAsync(paths));
        await LoadQueueAsync();
    }

    private async void ListLibrary_MouseDoubleClick(object sender, MouseButtonEventArgs e)
    {
        if (SelectedBrowse() is not BrowseItem item) return;
        if (item.Kind == BrowseKind.Track)
            await PlayBrowseItem(item);
        else
            EnterItem(item);
    }

    private void ListLibrary_MouseLeftButtonUp(object sender, MouseButtonEventArgs e)
    {
        if (e.ChangedButton != MouseButton.Left) return;
        if (SelectedBrowse() is not BrowseItem item) return;
        if (item.Kind == BrowseKind.Track) return;
        EnterItem(item);
    }



    private async void CtxPlay_Click(object sender, RoutedEventArgs e)
    {
        if (SelectedBrowse() is BrowseItem item)
            await PlayBrowseItem(item);
    }

    private async void CtxAddQueue_Click(object sender, RoutedEventArgs e)
    {
        if (SelectedBrowse() is BrowseItem item)
            await AddBrowseToQueue(item);
    }

    private async Task LoadQueueAsync()
    {
        try
        {
            var q = await _api.GetQueueAsync();
            _queue.Clear();
            if (q == null)
            {
                TxtQueueCount.Text = "— / 0";
                return;
            }
            for (int i = 0; i < q.Paths.Count; i++)
            {
                _queue.Add(new QueueItem
                {
                    Index = i,
                    Path = q.Paths[i],
                    IsCurrent = q.Index == i,
                });
            }
            if (q.Index is int idx && idx >= 0 && _queue.Count > 0)
                TxtQueueCount.Text = $"第 {idx + 1} / {_queue.Count} 首";
            else
                TxtQueueCount.Text = _queue.Count > 0 ? $"— / {_queue.Count} 首" : "— / 0";
        }
        catch (Exception ex)
        {
            SetStatus("播放列表加载失败: " + ex.Message);
        }
    }

    private async Task PlayQueueItem(QueueItem item)
    {
        var paths = _queue.Select(x => x.Path).ToList();
        if (paths.Count == 0) return;
        var start = Math.Clamp(item.Index, 0, paths.Count - 1);
        await RunCmd(() => _api.SetQueueAsync(paths, start));
        await LoadQueueAsync();
    }

    private async void ListQueue_MouseDoubleClick(object sender, MouseButtonEventArgs e)
    {
        if (ListQueue.SelectedItem is QueueItem item)
            await PlayQueueItem(item);
    }

    private async void CtxQueuePlay_Click(object sender, RoutedEventArgs e)
    {
        if (ListQueue.SelectedItem is QueueItem item)
            await PlayQueueItem(item);
    }

    private async void CtxQueueRemove_Click(object sender, RoutedEventArgs e)
    {
        if (ListQueue.SelectedItem is not QueueItem item)
        {
            SetStatus("请先选中一项");
            return;
        }
        await RunCmd(() => _api.QueueRemoveAsync(item.Index));
        await LoadQueueAsync();
    }

    private async void BtnPlayPause_Click(object sender, RoutedEventArgs e)
    {
        await RunCmd(async () =>
        {
            if (string.Equals(_lastState, "playing", StringComparison.OrdinalIgnoreCase))
                return await _api.PauseAsync();
            return await _api.ResumeAsync();
        });
    }

    private async void BtnStop_Click(object sender, RoutedEventArgs e)
        => await RunCmd(() => _api.StopAsync());

    private async void BtnNext_Click(object sender, RoutedEventArgs e)
        => await RunCmd(() => _api.NextAsync());

    private async void BtnPrev_Click(object sender, RoutedEventArgs e)
        => await RunCmd(() => _api.PrevAsync());

    private void SliderVolume_DragStarted(object sender, System.Windows.Controls.Primitives.DragStartedEventArgs e)
    {
        _draggingVolume = true;
    }

    private void SliderVolume_DragDelta(object sender, System.Windows.Controls.Primitives.DragDeltaEventArgs e)
    {
        TxtVolume.Text = ((int)SliderVolume.Value).ToString();
    }

    private async void SliderVolume_DragCompleted(object sender, System.Windows.Controls.Primitives.DragCompletedEventArgs e)
    {
        _draggingVolume = false;
        TxtVolume.Text = ((int)SliderVolume.Value).ToString();
        if (!_connected) return;
        try
        {
            await _api.SetVolumeAsync((int)SliderVolume.Value);
        }
        catch (Exception ex)
        {
            SetStatus("音量设置失败: " + ex.Message);
        }
    }

    private void SliderProgress_DragStarted(object sender, System.Windows.Controls.Primitives.DragStartedEventArgs e)
    {
        _draggingProgress = true;
    }

    private void SliderProgress_DragDelta(object sender, System.Windows.Controls.Primitives.DragDeltaEventArgs e)
    {
        TxtPos.Text = FmtTime(SliderProgress.Value);
    }

    private async void SliderProgress_DragCompleted(object sender, System.Windows.Controls.Primitives.DragCompletedEventArgs e)
    {
        _draggingProgress = false;
        await DoSeekAsync(SliderProgress.Value);
    }

    private async void SliderProgress_PreviewMouseLeftButtonUp(object sender, MouseButtonEventArgs e)
    {
        // 点击轨道跳转（IsMoveToPointEnabled 会先改 Value）
        if (_draggingProgress || !_connected) return;
        if (!SliderProgress.IsEnabled) return;
        await DoSeekAsync(SliderProgress.Value);
    }

    private async Task DoSeekAsync(double pos)
    {
        if (!_connected) return;
        try
        {
            var st = await _api.SeekAsync(pos);
            if (st != null)
                ApplyStatus(st);
            SetStatus($"跳转到 {FmtTime(pos)}");
        }
        catch (Exception ex)
        {
            SetStatus("跳转失败: " + ex.Message + "（需 mpv；mpg123 不支持精确 seek）");
        }
    }

    





    private async void BtnClearQueue_Click(object sender, RoutedEventArgs e)
    {
        await RunCmd(() => _api.QueueClearAsync());
        await LoadQueueAsync();
    }

    // ----- Menu -----

    private void MenuOpen_Click(object sender, RoutedEventArgs e)
        => MessageBox.Show(this, "打开【占位！！】", "文件", MessageBoxButton.OK, MessageBoxImage.Information);

    private void MenuExit_Click(object sender, RoutedEventArgs e) => Close();

    private void MenuSpeakerSettings_Click(object sender, RoutedEventArgs e)
    {
        var w = new SpeakerSettingsWindow(this) { Owner = this };
        w.ShowDialog();
    }

    private void MenuLibrarySettings_Click(object sender, RoutedEventArgs e)
    {
        if (!_connected)
        {
            MessageBox.Show(this, "请先连接服务器", "音乐库设置", MessageBoxButton.OK, MessageBoxImage.Information);
            return;
        }
        var w = new LibrarySettingsWindow(_api) { Owner = this };
        w.ShowDialog();
        // 不清导航栈，关设置后仍停在当前文件夹
        _ = LoadLibraryAsync();
    }

    private void MenuLanguageZh_Click(object sender, RoutedEventArgs e)
        => MessageBox.Show(this, "切换到中文【占位！！】", "语言", MessageBoxButton.OK, MessageBoxImage.Information);

    private void MenuLanguageEn_Click(object sender, RoutedEventArgs e)
        => MessageBox.Show(this, "Switch to English【placeholder】", "Language", MessageBoxButton.OK, MessageBoxImage.Information);

    private void MenuSupport_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
            {
                FileName = "https://github.com/setsuodu/RKSonos/",
                UseShellExecute = true
            });
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, "无法打开链接：\n" + ex.Message, "技术支持", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
    }

    private void MenuCheckUpdate_Click(object sender, RoutedEventArgs e)
        => MessageBox.Show(this, "检查更新【占位！！】", "帮助", MessageBoxButton.OK, MessageBoxImage.Information);

    private void MenuAbout_Click(object sender, RoutedEventArgs e)
    {
        var ver = System.Reflection.Assembly.GetExecutingAssembly().GetName().Version?.ToString(3) ?? "0.0.0";
        var srv = string.IsNullOrWhiteSpace(_serverVersion) ? "未知（请先连接）" : _serverVersion;
        MessageBox.Show(this,
            $"RKSonos Client  {ver}\n服务器版本  {srv}\n\nRK356X 实现 SONOS 音箱\nhttps://github.com/setsuodu/RKSonos/",
            "关于", MessageBoxButton.OK, MessageBoxImage.Information);
    }

    private async Task RunCmd(Func<Task<StatusResponse?>> action)
    {
        if (!_connected)
        {
            SetStatus("请先连接服务器");
            return;
        }
        if (_busy) return;
        _busy = true;
        try
        {
            var st = await action();
            if (st != null)
                ApplyStatus(st);
            SetStatus("OK");
        }
        catch (Exception ex)
        {
            SetStatus("操作失败: " + ex.Message);
        }
        finally
        {
            _busy = false;
        }
    }
}
