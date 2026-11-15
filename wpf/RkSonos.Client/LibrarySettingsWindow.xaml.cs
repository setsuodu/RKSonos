using System.Windows;
using RkSonos.Client.Models;
using RkSonos.Client.Services;

namespace RkSonos.Client;

public partial class LibrarySettingsWindow : Window
{
    private readonly RkSonosApi _api;
    private bool _busy;

    public LibrarySettingsWindow(RkSonosApi api)
    {
        InitializeComponent();
        _api = api;
        Loaded += async (_, _) => await RefreshAsync();
    }

    private void SetStatus(string msg) => TxtDlgStatus.Text = msg;

    private async Task RefreshAsync()
    {
        try
        {
            var list = await _api.GetMountsAsync();
            ListFolders.ItemsSource = list?.Mounts ?? new List<MountView>();
            if (list != null && !list.Gio)
                SetStatus("板子未检测到 gio，请安装 gvfs-backends / gvfs-fuse");
            else
                SetStatus($"共 {list?.Mounts?.Count ?? 0} 个文件夹");
        }
        catch (Exception ex)
        {
            SetStatus("加载失败: " + ex.Message);
        }
    }

    private async void BtnRefresh_Click(object sender, RoutedEventArgs e)
        => await RefreshAsync();

    private async void BtnAdd_Click(object sender, RoutedEventArgs e)
    {
        if (_busy) return;
        var dlg = new AddMountDialog { Owner = this };
        if (dlg.ShowDialog() != true) return;

        _busy = true;
        try
        {
            SetStatus("正在挂载...");
            await _api.AddMountAsync(
                uri: dlg.UriText,
                username: string.IsNullOrWhiteSpace(dlg.UserText) ? null : dlg.UserText,
                password: string.IsNullOrEmpty(dlg.PasswordText) ? null : dlg.PasswordText);
            SetStatus("已添加");
            await RefreshAsync();
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, ex.Message, "添加失败", MessageBoxButton.OK, MessageBoxImage.Warning);
            SetStatus("添加失败");
        }
        finally
        {
            _busy = false;
        }
    }

    private async void BtnRemove_Click(object sender, RoutedEventArgs e)
    {
        if (ListFolders.SelectedItem is not MountView m)
        {
            MessageBox.Show(this, "请先选中一项", "移除", MessageBoxButton.OK, MessageBoxImage.Information);
            return;
        }
        if (_busy) return;
        _busy = true;
        try
        {
            await _api.DeleteMountAsync(m.Id);
            await RefreshAsync();
            SetStatus("已移除");
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, ex.Message, "移除失败", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
        finally
        {
            _busy = false;
        }
    }

    private async void BtnRemount_Click(object sender, RoutedEventArgs e)
    {
        if (ListFolders.SelectedItem is not MountView m)
        {
            MessageBox.Show(this, "请先选中一项", "重挂", MessageBoxButton.OK, MessageBoxImage.Information);
            return;
        }
        if (_busy) return;
        _busy = true;
        try
        {
            SetStatus("重挂中...");
            await _api.RemountAsync(m.Id);
            await RefreshAsync();
            SetStatus("重挂完成");
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, ex.Message, "重挂失败", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
        finally
        {
            _busy = false;
        }
    }

    private void BtnClose_Click(object sender, RoutedEventArgs e) => Close();
}
