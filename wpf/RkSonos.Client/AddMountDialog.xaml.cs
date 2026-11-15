using System.Windows;

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

    private void BtnOk_Click(object sender, RoutedEventArgs e)
    {
        if (string.IsNullOrWhiteSpace(UriText))
        {
            MessageBox.Show(this, "请填写 SMB 路径（如 smb://host/share）", "添加",
                MessageBoxButton.OK, MessageBoxImage.Information);
            return;
        }
        DialogResult = true;
    }
}
