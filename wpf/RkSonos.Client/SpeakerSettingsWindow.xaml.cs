using System.Windows;
using System.Windows.Media;

namespace RkSonos.Client;

public partial class SpeakerSettingsWindow : Window
{
    private readonly MainWindow _owner;

    public SpeakerSettingsWindow(MainWindow owner)
    {
        InitializeComponent();
        _owner = owner;
        TxtServer.Text = owner.ServerUrl;
        SyncFromOwner();
    }

    public void SyncFromOwner()
    {
        TxtServer.Text = _owner.ServerUrl;
        var ok = _owner.IsConnected;
        DotStatus.Fill = ok
            ? (Brush)FindResource("OkBrush")
            : (Brush)FindResource("DangerBrush");
        TxtConn.Text = _owner.ConnectionDetail;
    }

    private async void BtnConnect_Click(object sender, RoutedEventArgs e)
    {
        BtnConnect.IsEnabled = false;
        try
        {
            await _owner.ConnectFromSettingsAsync(TxtServer.Text);
            SyncFromOwner();
        }
        finally
        {
            BtnConnect.IsEnabled = true;
        }
    }

    private async void BtnRefresh_Click(object sender, RoutedEventArgs e)
    {
        BtnRefresh.IsEnabled = false;
        try
        {
            await _owner.RefreshFromSettingsAsync();
            SyncFromOwner();
        }
        finally
        {
            BtnRefresh.IsEnabled = true;
        }
    }

    private void BtnSwitchDevice_Click(object sender, RoutedEventArgs e)
    {
        DialogResult = true;
        Close();
        _owner.SwitchDevice();
    }

    private void BtnClose_Click(object sender, RoutedEventArgs e) => Close();
}
