using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace OpenDM;

/// <summary>IDM's "Authorization Required": a user name and password for a download.</summary>
public sealed partial class LoginWindow : Window
{
    private Action<LoginAnswer?>? done;

    internal LoginWindow(string url, string name, string user, Action<LoginAnswer?> done)
    {
        this.done = done;
        InitializeComponent();
        var host = SiteLogins.SiteOf(url);
        Heading.Text = $"{host} asks for a user name and password";
        NameText.Text = name;
        UserBox.Text = user;
        RememberBox.Content = $"Remember for {host}";
        LogInButton.IsEnabled = user.Trim().Length > 0;
        ToolWindows.Setup(this, 440, topmost: true);
        ToolWindows.Keys(this, enter: LogIn, escape: () => Finish(null));
        Closed += (_, _) => Answer(null);
        // The password is what is missing when the user name is known.
        (user.Length > 0 ? (Control)PasswordBox : UserBox).Loaded += (sender, _) => ((Control)sender).Focus(FocusState.Programmatic);
    }

    private void UserBox_TextChanged(object sender, TextChangedEventArgs e) => LogInButton.IsEnabled = UserBox.Text.Trim().Length > 0;

    private void LogIn_Click(object sender, RoutedEventArgs e) => LogIn();
    private void Cancel_Click(object sender, RoutedEventArgs e) => Finish(null);

    private void LogIn()
    {
        if (!LogInButton.IsEnabled) return;
        Finish(new LoginAnswer(UserBox.Text.Trim(), PasswordBox.Password, RememberBox.IsChecked == true));
    }

    private void Finish(LoginAnswer? answer)
    {
        Answer(answer);
        Close();
    }

    /// <summary>Answers once; the close button is Cancel.</summary>
    private void Answer(LoginAnswer? answer)
    {
        var callback = done;
        done = null;
        callback?.Invoke(answer);
    }
}
