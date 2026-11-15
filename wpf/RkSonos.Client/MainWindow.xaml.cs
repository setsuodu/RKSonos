using System.Collections.ObjectModel;
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
    private readonly ObservableCollection<Track> _tracks = new();
    private readonly ObservableCollection<QueueItem> _queue = new();

    private bool _connected;
    private bool _suppressVolume;
    private bool _suppressMode;
    private bool _busy;
    private string _lastState = "idle";

    public MainWindow() : this("http://127.0.0.1:8080") { }

    public MainWindow(string serverBaseUrl)
    {
        InitializeComponent();
        ListLibrary.ItemsSource = _tracks;
        ListQueue.ItemsSource = _queue;
        TxtServer.Text = serverBaseUrl;

        _pollTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(2) };
        _pollTimer.Tick += async (_, _) =>
        {
            if (_connected && !_busy)
                await SafeRefreshStatusAsync();
        };

        Loaded += async (_, _) =>
        {
            await ConnectAsync();
        };

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
        DotStatus.Fill = ok
            ? (Brush)FindResource("OkBrush")
            : (Brush)FindResource("DangerBrush");
        TxtConn.Text = detail;
        if (ok)
            _pollTimer.Start();
        else
            _pollTimer.Stop();
    }

    private async void BtnConnect_Click(object sender, RoutedEventArgs e)
    {
        await ConnectAsync();
    }

    private async Task ConnectAsync()
    {
        if (_busy) return;
        _busy = true;
        try
        {
            _api.BaseUrl = TxtServer.Text;
            SetStatus($"连接 {_api.BaseUrl} ...");
            var health = await _api.HealthAsync();
            if (!string.Equals(health, "ok", StringComparison.OrdinalIgnoreCase))
                throw new Exception($"health={health}");

            SetConnected(true, "已连接");
            SetStatus($"已连接 {_api.BaseUrl}");
            await RefreshAllAsync();
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
    }

    private async void BtnRefresh_Click(object sender, RoutedEventArgs e)
    {
        if (!_connected)
        {
            await ConnectAsync();
            return;
        }
        await RefreshAllAsync();
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
            if (st == null) return;
            ApplyStatus(st);
        }
        catch (Exception ex)
        {
            SetConnected(false, "断开");
            SetStatus("状态刷新失败: " + ex.Message);
        }
    }

    private void ApplyStatus(StatusResponse st)
    {
        _lastState = st.State ?? "idle";
        var name = string.IsNullOrWhiteSpace(st.Path) ? "—" : System.IO.Path.GetFileName(st.Path);
        TxtNowPlaying.Text = name;
        TxtMeta.Text = $"状态: {st.State}  ·  模式: {st.Mode}  ·  队列: {(st.QueueIndex is int qi ? qi + 1 : 0)}/{st.QueueLen}  ·  {st.Path}";

        BtnPlayPause.Content = string.Equals(st.State, "playing", StringComparison.OrdinalIgnoreCase)
            ? "⏸ 暂停"
            : "▶ 播放/继续";

        _suppressVolume = true;
        SliderVolume.Value = Math.Clamp(st.Volume, 0, 100);
        TxtVolume.Text = ((int)SliderVolume.Value).ToString();
        _suppressVolume = false;

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
        if (BarProgress == null) return;
        var pos = position ?? 0;
        var dur = duration ?? 0;
        TxtPos.Text = FmtTime(pos);
        TxtDur.Text = dur > 0 ? FmtTime(dur) : "0:00";
        if (dur > 0.5)
        {
            BarProgress.IsIndeterminate = false;
            BarProgress.Maximum = dur;
            BarProgress.Value = Math.Clamp(pos, 0, dur);
        }
        else
        {
            // mpg123 等无 duration：播放中跑不确定条
            BarProgress.IsIndeterminate = string.Equals(_lastState, "playing", StringComparison.OrdinalIgnoreCase);
            BarProgress.Value = 0;
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
            var uri = new Uri(baseUrl + "/ws");
            await ws.ConnectAsync(uri, ct);
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
                    st = JsonSerializer.Deserialize<StatusResponse>(json, new JsonSerializerOptions
                    {
                        PropertyNameCaseInsensitive = true,
                    });
                }
                catch { /* ignore bad frame */ }
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

    private void SelectMode(string mode)
    {
        mode = (mode ?? "normal").ToLowerInvariant();
        for (int i = 0; i < CmbMode.Items.Count; i++)
        {
            if (CmbMode.Items[i] is ComboBoxItem item &&
                string.Equals(item.Tag?.ToString(), mode, StringComparison.OrdinalIgnoreCase))
            {
                CmbMode.SelectedIndex = i;
                return;
            }
        }
        CmbMode.SelectedIndex = 0;
    }

    private async Task LoadLibraryAsync(string? q = null)
    {
        try
        {
            var libSt = await _api.GetLibraryStatusAsync();
            if (libSt != null)
            {
                TxtLibInfo.Text = libSt.Scanning
                    ? $"扫描中… 已有 {libSt.TrackCount}"
                    : $"{libSt.TrackCount} 首 · root={libSt.Root}";
            }

            var tracks = await _api.GetLibraryAsync(q, limit: 500, offset: 0);
            _tracks.Clear();
            foreach (var t in tracks)
                _tracks.Add(t);
            SetStatus($"曲库加载 {tracks.Count} 首");
        }
        catch (Exception ex)
        {
            SetStatus("曲库加载失败: " + ex.Message);
        }
    }

    private async Task LoadQueueAsync()
    {
        try
        {
            var q = await _api.GetQueueAsync();
            _queue.Clear();
            if (q == null) return;
            for (int i = 0; i < q.Paths.Count; i++)
            {
                _queue.Add(new QueueItem
                {
                    Index = i,
                    Path = q.Paths[i],
                    IsCurrent = q.Index == i,
                });
            }
        }
        catch (Exception ex)
        {
            SetStatus("队列加载失败: " + ex.Message);
        }
    }

    private async void BtnPlayPause_Click(object sender, RoutedEventArgs e)
    {
        await RunCmd(async () =>
        {
            if (string.Equals(_lastState, "playing", StringComparison.OrdinalIgnoreCase))
                return await _api.PauseAsync();
            if (string.Equals(_lastState, "paused", StringComparison.OrdinalIgnoreCase))
                return await _api.ResumeAsync();
            // idle/stopped: try resume first, else do nothing meaningful
            return await _api.ResumeAsync();
        });
    }

    private async void BtnStop_Click(object sender, RoutedEventArgs e)
        => await RunCmd(() => _api.StopAsync());

    private async void BtnNext_Click(object sender, RoutedEventArgs e)
        => await RunCmd(() => _api.NextAsync());

    private async void BtnPrev_Click(object sender, RoutedEventArgs e)
        => await RunCmd(() => _api.PrevAsync());

    private async void SliderVolume_ValueChanged(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        if (_suppressVolume || !_connected) return;
        TxtVolume.Text = ((int)SliderVolume.Value).ToString();
        // debounce lightly via timer would be nicer; keep simple
        try
        {
            await _api.SetVolumeAsync((int)SliderVolume.Value);
        }
        catch (Exception ex)
        {
            SetStatus("音量设置失败: " + ex.Message);
        }
    }

    private async void CmbMode_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_suppressMode || !_connected) return;
        if (CmbMode.SelectedItem is not ComboBoxItem item) return;
        var mode = item.Tag?.ToString() ?? "normal";
        await RunCmd(() => _api.SetModeAsync(mode));
    }

    private async void BtnSearch_Click(object sender, RoutedEventArgs e)
        => await LoadLibraryAsync(TxtSearch.Text.Trim());

    private async void TxtSearch_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key == Key.Enter)
            await LoadLibraryAsync(TxtSearch.Text.Trim());
    }

    private async void BtnLoadLib_Click(object sender, RoutedEventArgs e)
        => await LoadLibraryAsync(string.IsNullOrWhiteSpace(TxtSearch.Text) ? null : TxtSearch.Text.Trim());

    private async void BtnScan_Click(object sender, RoutedEventArgs e)
    {
        await RunCmd(async () =>
        {
            SetStatus("扫库中…（可能较慢）");
            var r = await _api.LibraryScanAsync();
            SetStatus(r == null ? "扫库完成" : $"扫库完成: {r.Count} 首, {r.TookMs}ms");
            await LoadLibraryAsync();
            return await _api.GetStatusAsync();
        });
    }

    private Track? SelectedTrack() => ListLibrary.SelectedItem as Track;

    private async void BtnLibraryPlay_Click(object sender, RoutedEventArgs e)
    {
        var t = SelectedTrack();
        if (t == null)
        {
            SetStatus("请先在曲库中选中一首");
            return;
        }
        await RunCmd(() => _api.LibraryPlayAsync(path: t.Path));
        await LoadQueueAsync();
    }

    private async void BtnSinglePlay_Click(object sender, RoutedEventArgs e)
    {
        var t = SelectedTrack();
        if (t == null)
        {
            SetStatus("请先在曲库中选中一首");
            return;
        }
        await RunCmd(() => _api.PlayAsync(t.Path));
        await LoadQueueAsync();
    }

    private async void BtnAddToQueue_Click(object sender, RoutedEventArgs e)
    {
        var t = SelectedTrack();
        if (t == null)
        {
            SetStatus("请先在曲库中选中一首");
            return;
        }
        await RunCmd(() => _api.QueueAddAsync(new[] { t.Path }));
        await LoadQueueAsync();
    }

    private async void ListLibrary_MouseDoubleClick(object sender, MouseButtonEventArgs e)
    {
        if (SelectedTrack() != null)
            await BtnLibraryPlay_Click_Run();
    }

    private async Task BtnLibraryPlay_Click_Run()
    {
        var t = SelectedTrack();
        if (t == null) return;
        await RunCmd(() => _api.LibraryPlayAsync(path: t.Path));
        await LoadQueueAsync();
    }

    private async void BtnRefreshQueue_Click(object sender, RoutedEventArgs e)
        => await LoadQueueAsync();

    private async void BtnRemoveQueue_Click(object sender, RoutedEventArgs e)
    {
        if (ListQueue.SelectedItem is not QueueItem item)
        {
            SetStatus("请先选中队列中的一项");
            return;
        }
        await RunCmd(() => _api.QueueRemoveAsync(item.Index));
        await LoadQueueAsync();
    }

    private async void BtnClearQueue_Click(object sender, RoutedEventArgs e)
    {
        await RunCmd(() => _api.QueueClearAsync());
        await LoadQueueAsync();
    }

    private void BtnSwitchDevice_Click(object sender, RoutedEventArgs e)
    {
        var disc = new DiscoveryWindow();
        disc.Show();
        Close();
    }


    // ----- Menu -----

    private void MenuOpen_Click(object sender, RoutedEventArgs e)
        => MessageBox.Show(this, "打开【占位！！】", "文件", MessageBoxButton.OK, MessageBoxImage.Information);

    private void MenuExit_Click(object sender, RoutedEventArgs e)
        => Close();

    private void MenuLibrarySettings_Click(object sender, RoutedEventArgs e)
    {
        if (!_connected)
        {
            MessageBox.Show(this, "请先连接服务器", "音乐库设置", MessageBoxButton.OK, MessageBoxImage.Information);
            return;
        }
        var w = new LibrarySettingsWindow(_api) { Owner = this };
        w.ShowDialog();
        // 设置里可能改了挂载，刷新曲库列表
        _ = LoadLibraryAsync();
    }

    private void MenuLanguage_Click(object sender, RoutedEventArgs e)
        => MessageBox.Show(this, "语言【占位！！】", "设置", MessageBoxButton.OK, MessageBoxImage.Information);

    private void MenuSupport_Click(object sender, RoutedEventArgs e)
        => MessageBox.Show(this, "技术支持网站【占位！！】", "帮助", MessageBoxButton.OK, MessageBoxImage.Information);

    private void MenuCheckUpdate_Click(object sender, RoutedEventArgs e)
        => MessageBox.Show(this, "检查更新【占位！！】", "帮助", MessageBoxButton.OK, MessageBoxImage.Information);

    private void MenuAbout_Click(object sender, RoutedEventArgs e)
        => MessageBox.Show(this, "RKSonos Client\n关于【占位！！】", "关于", MessageBoxButton.OK, MessageBoxImage.Information);

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
