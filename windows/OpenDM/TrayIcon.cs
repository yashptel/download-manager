using H.NotifyIcon;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media.Imaging;

namespace OpenDM;

/// <summary>
/// Notification-area icon: total speed in the tooltip, and quick controls.
/// The native popup menu runs each item's Command, so items use commands
/// rather than Click handlers.
/// </summary>
internal sealed class TrayIcon : IDisposable
{
    private readonly TaskbarIcon icon;

    public TrayIcon(DownloadStore store, DownloadCore core, Action showWindow, Action quit)
    {
        var show = Command(showWindow);
        var menu = new MenuFlyout();
        menu.Items.Add(new MenuFlyoutItem { Text = "Show downloads", Command = show });
        menu.Items.Add(new MenuFlyoutSeparator());
        menu.Items.Add(new MenuFlyoutItem { Text = "Resume all", Command = Command(core.ResumeAll) });
        menu.Items.Add(new MenuFlyoutItem { Text = "Pause all", Command = Command(core.PauseAll) });
        menu.Items.Add(new MenuFlyoutSeparator());
        menu.Items.Add(new MenuFlyoutItem { Text = "Exit", Command = Command(quit) });

        icon = new TaskbarIcon
        {
            ToolTipText = "OpenDM",
            IconSource = new BitmapImage(new Uri(Path.Combine(AppContext.BaseDirectory, "Assets", "OpenDM.ico"))),
            ContextFlyout = menu,
            LeftClickCommand = show,
            NoLeftClickDelay = true,
        };
        // Efficiency mode would throttle the process, and with it the downloads.
        icon.ForceCreate(enablesEfficiencyMode: false);

        store.StatusChanged += () =>
        {
            var stats = store.Stats;
            var text = stats.Active > 0
                ? $"OpenDM \u2014 {stats.Active} downloading, {Format.Speed(stats.DownloadSpeed)}"
                : "OpenDM";
            if (icon.ToolTipText != text) icon.ToolTipText = text;
        };
    }

    public void Dispose() => icon.Dispose();

    private static XamlUICommand Command(Action action)
    {
        var command = new XamlUICommand();
        command.ExecuteRequested += (_, _) => action();
        return command;
    }
}
