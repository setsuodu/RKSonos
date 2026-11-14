using System.Collections.ObjectModel;
using System.Net.Http;
using System.Windows;
using System.Windows.Input;
using RkSonos.Client.Services;

namespace RkSonos.Client;

public partial class DiscoveryWindow : Window
{
    private readonly ObservableCollection<DiscoveredDevice> _devices = new();
    private CancellationTokenSource? _scanCts;
    private bool _scanning;

    public DiscoveryWindow()
    {
        InitializeComponent();
        ListDevices.ItemsSource = _devices;
        Loaded += async (_, _) => await ScanAsync();
        Closed += (_, _) => _scanCts?.Cancel();
    }

    private async void BtnScan_Click(object sender, RoutedEventArgs e) => await ScanAsync();

    private async Task ScanAsync()
    {
        if (_scanning) return;
        _scanning = true;
        _scanCts?.Cancel();
        _scanCts = new CancellationTokenSource();
        var ct = _scanCts.Token;

        BtnScan.IsEnabled = false;
        TxtScanState.Text = "扫描中…";
        TxtHint.Text = "正在局域网查找设备（mDNS _rksonos._tcp + HTTP 探测）…";
        _devices.Clear();

        try
        {
            await MdnsDiscovery.BrowseAsync(_devices, ct);
            if (_devices.Count == 0)
            {
                TxtHint.Text = "未发现设备。请确认板上 server 已启动且同一局域网，或使用下方手动连接。";
                TxtScanState.Text = "无结果";
                TxtStatus.Text = "仍停留在找设备页，不会进入播放页。";
            }
            else
            {
                TxtHint.Text = $"发现 {_devices.Count} 台设备，双击或点「使用选中的设备」。";
                TxtScanState.Text = $"找到 {_devices.Count}";
                ListDevices.SelectedIndex = 0;
            }
        }
        catch (OperationCanceledException)
        {
            TxtScanState.Text = "已取消";
        }
        catch (Exception ex)
        {
            TxtScanState.Text = "扫描出错";
            TxtStatus.Text = ex.Message;
        }
        finally
        {
            _scanning = false;
            BtnScan.IsEnabled = true;
        }
    }

    private async void BtnUseSelected_Click(object sender, RoutedEventArgs e)
    {
        if (ListDevices.SelectedItem is not DiscoveredDevice d)
        {
            TxtStatus.Text = "请先在列表中选中一台设备。";
            return;
        }
        await OpenPlayerAsync(d.Address);
    }

    private async void ListDevices_MouseDoubleClick(object sender, MouseButtonEventArgs e)
    {
        if (ListDevices.SelectedItem is DiscoveredDevice d)
            await OpenPlayerAsync(d.Address);
    }

    private async void BtnManual_Click(object sender, RoutedEventArgs e)
    {
        var url = (TxtManual.Text ?? "").Trim();
        if (string.IsNullOrWhiteSpace(url))
        {
            TxtStatus.Text = "请填写地址，例如 http://192.168.1.50:8080";
            return;
        }
        await OpenPlayerAsync(url);
    }

    private async Task OpenPlayerAsync(string baseUrl)
    {
        TxtStatus.Text = $"检查 {baseUrl} …";
        try
        {
            if (!baseUrl.StartsWith("http://", StringComparison.OrdinalIgnoreCase) &&
                !baseUrl.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
                baseUrl = "http://" + baseUrl;
            baseUrl = baseUrl.TrimEnd('/');

            using var http = new HttpClient { Timeout = TimeSpan.FromSeconds(3) };
            var body = (await http.GetStringAsync(baseUrl + "/health")).Trim();
            if (!body.Equals("ok", StringComparison.OrdinalIgnoreCase))
                throw new Exception($"health={body}");

            var main = new MainWindow(baseUrl);
            main.Show();
            Close();
        }
        catch (Exception ex)
        {
            TxtStatus.Text = "无法连接，仍留在找设备页: " + ex.Message;
        }
    }
}
