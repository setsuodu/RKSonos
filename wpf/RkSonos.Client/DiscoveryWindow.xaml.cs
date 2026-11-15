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
        // 输入框默认留空；上次地址只在后台探测，不预填
        Loaded += async (_, _) =>
        {
            UpdateManualPlaceholder();
            await ScanAsync();
        };
        Closed += (_, _) => _scanCts?.Cancel();
    }

    private void TxtManual_TextChanged(object sender, System.Windows.Controls.TextChangedEventArgs e)
        => UpdateManualPlaceholder();

    private void UpdateManualPlaceholder()
    {
        if (TxtManualPlaceholder == null) return;
        TxtManualPlaceholder.Visibility = string.IsNullOrEmpty(TxtManual.Text)
            ? Visibility.Visible
            : Visibility.Collapsed;
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
        TxtScanState.Text = "正在搜索…";
        TxtStatus.Text = "";
        _devices.Clear();

        var hints = new List<string>();
        // 用户正在输入的地址
        if (!string.IsNullOrWhiteSpace(TxtManual.Text))
            hints.Add(TxtManual.Text.Trim());
        // 上次成功连接的地址（不显示在输入框，只后台优先探测）
        var last = LastServerStore.Load();
        if (!string.IsNullOrWhiteSpace(last) &&
            !hints.Contains(last, StringComparer.OrdinalIgnoreCase))
            hints.Add(last);

        // 进度文案对用户保持简短，不暴露 mDNS / health 细节
        var progress = new Progress<string>(_ => { TxtScanState.Text = "正在搜索…"; });

        try
        {
            await MdnsDiscovery.BrowseAsync(_devices, ct, hints, progress: progress);

            if (_devices.Count == 0)
            {
                TxtScanState.Text = "未找到音箱";
                TxtStatus.Text = "请确认音箱已开机，并与电脑在同一 Wi‑Fi，或在下方手动输入地址。";
            }
            else
            {
                TxtScanState.Text = $"找到 {_devices.Count} 台";
                ListDevices.SelectedIndex = 0;
                TxtStatus.Text = "";
            }
        }
        catch (OperationCanceledException)
        {
            TxtScanState.Text = "已取消";
        }
        catch (Exception)
        {
            TxtScanState.Text = "搜索失败";
            TxtStatus.Text = "请稍后点「刷新」重试，或手动输入地址。";
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
            TxtStatus.Text = "请先点选一台音箱。";
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
            TxtStatus.Text = "请输入地址，例如 http://192.168.1.252:8080";
            return;
        }
        await OpenPlayerAsync(url);
    }

    private async Task OpenPlayerAsync(string baseUrl)
    {
        TxtStatus.Text = "正在连接…";
        try
        {
            var ok = await LastServerStore.TryReachAsync(baseUrl, TimeSpan.FromSeconds(3));
            if (ok == null)
                throw new Exception("无法访问音箱");

            LastServerStore.Save(ok);
            var main = new MainWindow(ok);
            Application.Current.MainWindow = main;
            main.Show();
            Close();
        }
        catch (Exception)
        {
            TxtStatus.Text = "连接失败，请确认音箱已开机且地址正确。";
        }
    }
}
