using System.Windows;
using System.Windows.Interop;

namespace RkSonos.Client;

public partial class AddMountDialog : Window
{
    public string UriText => (TxtUri.Text ?? "").Trim();
    public string UserText => (TxtUser.Text ?? "").Trim();
    public string PasswordText => TxtPass.Password ?? "";

    public AddMountDialog()
    {
        InitializeComponent();
        Loaded += (_, _) => TxtUri.Focus();
    }

    private void BtnBrowse_Click(object sender, RoutedEventArgs e)
    {
        using var dlg = new System.Windows.Forms.FolderBrowserDialog
        {
            Description = "选择联网共享中的音乐文件夹",
            UseDescriptionForTitle = true,
            ShowNewFolderButton = false,
        };

        var cur = UriText.Replace('/', '\\');
        if (cur.StartsWith(@"\\", StringComparison.Ordinal) && System.IO.Directory.Exists(cur))
            dlg.SelectedPath = cur;

        var helper = new WindowInteropHelper(this);
        System.Windows.Forms.DialogResult result;
        if (helper.Handle != IntPtr.Zero)
            result = dlg.ShowDialog(new Win32Window(helper.Handle));
        else
            result = dlg.ShowDialog();

        if (result != System.Windows.Forms.DialogResult.OK)
            return;

        var path = (dlg.SelectedPath ?? "").Trim();
        if (path.Length == 0)
            return;

        TxtUri.Text = path;

        if (path.Length >= 2 && path[1] == ':' && !path.StartsWith(@"\\", StringComparison.Ordinal))
        {
            MessageBox.Show(this,
                "选的是本机路径。请到“网络”里选 \\\\服务器\\共享。",
                "提示", MessageBoxButton.OK, MessageBoxImage.Information);
        }
    }

    private void BtnOk_Click(object sender, RoutedEventArgs e)
    {
        if (string.IsNullOrWhiteSpace(UriText))
        {
            MessageBox.Show(this, "请填写或浏览选择共享路径", "添加",
                MessageBoxButton.OK, MessageBoxImage.Information);
            return;
        }
        DialogResult = true;
    }

    private sealed class Win32Window : System.Windows.Forms.IWin32Window
    {
        public IntPtr Handle { get; }
        public Win32Window(IntPtr handle) => Handle = handle;
    }
}
