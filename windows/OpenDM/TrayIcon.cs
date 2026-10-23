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
    private readonly Func<IntPtr> mainWindowHandle;
    private readonly UISettings settings = new();
    private readonly MenuFlyoutItem statusItem = new() { IsEnabled = false };
    private readonly MenuFlyoutItem resumeAllItem;
    private readonly MenuFlyoutItem pauseAllItem;
    private readonly BitmapImage lightImage = Asset("Tray-light.ico");
    private readonly BitmapImage darkImage = Asset("Tray-dark.ico");
    private readonly BitmapImage[] lightProgress = Enumerable.Range(0, 26).Select(k => Asset($"Tray-light-progress-{k}.ico")).ToArray();
    private readonly BitmapImage[] darkProgress = Enumerable.Range(0, 26).Select(k => Asset($"Tray-dark-progress-{k}.ico")).ToArray();
    private readonly DispatcherQueueTimer completionHold;
    private bool? lightTaskbar;
    private GlyphState glyphState = new GlyphState.Idle();

    public TrayIcon(DownloadStore store, DownloadCore core, Func<IntPtr> mainWindowHandle, Action showWindow, Action newDownload, Action quit)
    {
        this.mainWindowHandle = mainWindowHandle;
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
        completionHold = DispatcherQueue.GetForCurrentThread().CreateTimer();
        completionHold.Interval = TimeSpan.FromSeconds(1);
        completionHold.IsRepeating = false;
        completionHold.Tick += (_, _) =>
        {
            completionHold.Stop();
            glyphState = GlyphState.From(store.Items.Select(item => item.Row));
            ShowIcon();
            UpdateTaskbar(glyphState);
        };
        FollowTaskbarTheme();
        // Efficiency mode would throttle the process, and with it the downloads.
        icon.ForceCreate(enablesEfficiencyMode: false);
        var dispatcher = DispatcherQueue.GetForCurrentThread();
        settings.ColorValuesChanged += (_, _) => dispatcher.TryEnqueue(FollowTaskbarTheme);

        store.StatusChanged += () => Update(store);
        store.RowsChanged += changes => Update(store, changes.Any(change =>
            change.Old?.Status == TaskStatus.Downloading && change.Item.Row.Status == TaskStatus.Complete));
        store.RowsRemoved += _ => Update(store);
        Update(store);
    }

    public void Dispose()
    {
        completionHold.Stop();
        icon.Dispose();
    }

    private void Update(DownloadStore store, bool completed = false)
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
        UpdateGlyph(store, completed);
    }

    private void FollowTaskbarTheme()
    {
        // The taskbar follows the "Windows mode" setting, which can differ from the app mode.
        using var key = Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        var light = key?.GetValue("SystemUsesLightTheme") is int value && value != 0;
        if (light == lightTaskbar) return;
        lightTaskbar = light;
        ShowIcon();
    }

    private void ShowIcon()
    {
        var light = lightTaskbar == true;
        var image = glyphState switch
        {
            GlyphState.Idle => light ? lightImage : darkImage,
            _ => (light ? lightProgress : darkProgress)[GlyphState.FrameIndex(glyphState)],
        };
        if (icon.IconSource != image) icon.IconSource = image;
    }

    private void UpdateGlyph(DownloadStore store, bool completed)
    {
        var next = GlyphState.From(store.Items.Select(item => item.Row));
        if (completionHold.IsRunning)
        {
            if (next is GlyphState.Downloading)
            {
                completionHold.Stop();
            }
            else
            {
                return;
            }
        }
        if (completed && glyphState.IsDownloading && next is not GlyphState.Downloading)
        {
            glyphState = new GlyphState.Completed();
            ShowIcon();
            UpdateTaskbar(glyphState);
            completionHold.Start();
            return;
        }
        glyphState = next;
        ShowIcon();
        UpdateTaskbar(glyphState);
    }

    private void UpdateTaskbar(GlyphState state)
    {
        var window = mainWindowHandle();
        if (window == IntPtr.Zero) return;
        switch (state)
        {
            case GlyphState.Downloading downloading:
                Native.SetTaskbarProgress(window, Native.TaskbarProgressState.Normal, downloading.Fraction);
                break;
            case GlyphState.Paused:
                Native.SetTaskbarProgress(window, Native.TaskbarProgressState.Paused);
                break;
            default:
                Native.SetTaskbarProgress(window, Native.TaskbarProgressState.NoProgress);
                break;
        }
    }

    private static BitmapImage Asset(string file) =>
        new(new Uri(Path.Combine(AppContext.BaseDirectory, "Assets", file)));

    private static XamlUICommand Command(Action action)
    {
        var command = new XamlUICommand();
        command.ExecuteRequested += (_, _) => action();
        return command;
    }
}
