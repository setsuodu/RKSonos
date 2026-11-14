using System.Windows;
using RkSonos.Client.Services;

namespace RkSonos.Client;

public partial class App : Application
{
    protected override async void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);
        ShutdownMode = ShutdownMode.OnMainWindowClose;

        // 1) 有上次地址且 /health 通 → 直接进播放页，不扫
        var last = LastServerStore.Load();
        var ok = await LastServerStore.TryReachAsync(last);
        if (ok != null)
        {
            var main = new MainWindow(ok);
            MainWindow = main;
            main.Show();
            return;
        }

        // 2) 上次不可达（或没有记录）→ 找设备页
        var disc = new DiscoveryWindow();
        MainWindow = disc;
        disc.Show();
    }
}
