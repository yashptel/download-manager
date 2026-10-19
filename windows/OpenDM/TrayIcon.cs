using H.NotifyIcon;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.Win32;
using Windows.UI.ViewManagement;

namespace OpenDM;

/// <summary>
/// Notification-area icon: total speed in the tooltip, and quick controls.
/// Like the system's own tray icons it is a single color that follows the
/// taskbar's light or dark mode. The native popup menu runs each item's
/// Command, so items use commands rather than Click handlers.
/// </summary>
internal sealed partial class TrayIcon : IDisposable
{
    private readonly TaskbarIcon icon;
    private readonly UISettings settings = new();
    private readonly MenuFlyoutItem statusItem = new() { IsEnabled = false };
    private readonly MenuFlyoutItem resumeAllItem;
    private readonly MenuFlyoutItem pauseAllItem;
    private bool? lightTaskbar;

    public TrayIcon(DownloadStore store, DownloadCore core, Action showWindow, Action newDownload, Action quit)
    {
        var show = Command(showWindow);
        resumeAllItem = new MenuFlyoutItem { Text = "Resume all", Command = Command(core.ResumeAll) };
        pauseAllItem = new MenuFlyoutItem { Text = "Pause all", Command = Command(core.PauseAll) };
        var menu = new MenuFlyout();
        menu.Items.Add(new MenuFlyoutItem { Text = "Show downloads", Command = show });
        menu.Items.Add(new MenuFlyoutItem { Text = "New download\u2026", Command = Command(newDownload) });
        menu.Items.Add(new MenuFlyoutSeparator());
        menu.Items.Add(statusItem);
        menu.Items.Add(resumeAllItem);
        menu.Items.Add(pauseAllItem);
        menu.Items.Add(new MenuFlyoutSeparator());
        menu.Items.Add(new MenuFlyoutItem { Text = "Settings\u2026", Command = Command(() => SettingsWindow.Show(core)) });
        menu.Items.Add(new MenuFlyoutItem { Text = "Exit", Command = Command(quit) });

        icon = new TaskbarIcon
        {
            ToolTipText = "OpenDM",
            ContextFlyout = menu,
            LeftClickCommand = show,
            NoLeftClickDelay = true,
        };
        FollowTaskbarTheme();
        // Efficiency mode would throttle the process, and with it the downloads.
        icon.ForceCreate(enablesEfficiencyMode: false);
        var dispatcher = DispatcherQueue.GetForCurrentThread();
        settings.ColorValuesChanged += (_, _) => dispatcher.TryEnqueue(FollowTaskbarTheme);

        store.StatusChanged += () => Update(store);
        store.RowsChanged += _ => Update(store);
        store.RowsRemoved += _ => Update(store);
        Update(store);
    }

    public void Dispose() => icon.Dispose();

    private void Update(DownloadStore store)
    {
        var stats = store.Stats;
        var text = stats.Active > 0
            ? $"OpenDM \u2014 {stats.Active} downloading, {Format.Speed(stats.DownloadSpeed)}"
            : "OpenDM";
        if (icon.ToolTipText != text) icon.ToolTipText = text;
        var idle = Format.Idle(store.Items);
        statusItem.Text = (stats.Active, idle.Length) switch
        {
            (0, 0) => "No active downloads",
            (0, _) => idle,
            (var active, _) when stats.DownloadSpeed > 0 => $"{active} downloading \u00B7 {Format.Speed(stats.DownloadSpeed)}",
            (var active, _) => $"{active} downloading",
        };
        pauseAllItem.IsEnabled = stats.Active > 0 || store.Items.Any(item => item.Row.Status == TaskStatus.Queued);
        resumeAllItem.IsEnabled = store.Items.Any(item => item.Row.Status == TaskStatus.Paused);
    }

    private void FollowTaskbarTheme()
    {
        // The taskbar follows the "Windows mode" setting, which can differ from the app mode.
        using var key = Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        var light = key?.GetValue("SystemUsesLightTheme") is int value && value != 0;
        if (light == lightTaskbar) return;
        lightTaskbar = light;
        var file = light ? "Tray-light.ico" : "Tray-dark.ico";
        icon.IconSource = new BitmapImage(new Uri(Path.Combine(AppContext.BaseDirectory, "Assets", file)));
    }

    private static XamlUICommand Command(Action action)
    {
        var command = new XamlUICommand();
        command.ExecuteRequested += (_, _) => action();
        return command;
    }
}
