using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using WinRT.Interop;

namespace OpenDM;

public partial class App : Application
{
    private static App? current;
    private DispatcherQueue? dispatcher;
    private DownloadCore? core;
    private MainWindow? window;
    private TrayIcon? tray;
    private DispatcherQueueTimer? updateTimer;

    public App()
    {
        current = this;
        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        dispatcher = DispatcherQueue.GetForCurrentThread();
        var config = Config();
        // A second OpenDM on the same data folder would start a second engine on the same files.
        if (OpendmCoreMethods.AnotherInstanceRunning(config.DataDir))
        {
            Exit();
            return;
        }
        var store = new DownloadStore(dispatcher);
        try
        {
            core = new DownloadCore(config, store.Listener);
        }
        catch (Exception error)
        {
            Native.ShowError("OpenDM could not open its data", error.Message);
            Exit();
            return;
        }
        store.Load(core);
        store.BrowserDownloadsReceived += (downloads, _) => NewDownloads.Present(downloads, store, core);
        store.LoginNeeded += (id, url) => LoginPrompt.Handle(id, url, store, core);
        store.NewAddressReceived += (id, download) => AddressRefresh.Received(id, download, store, core);
        window = new MainWindow(store, core);
        tray = new TrayIcon(store, core,
            () => window is null ? IntPtr.Zero : WindowNative.GetWindowHandle(window),
            ShowMainWindow, () => window?.NewDownload(), Quit);
        core.Start();
        InstallBrowserHost(core);
        // The browser extension starts OpenDM with --background when it is not running.
        if (!Environment.GetCommandLineArgs().Contains("--background"))
        {
            window.ShowAndActivate();
        }
        updateTimer = dispatcher.CreateTimer();
        updateTimer.Interval = TimeSpan.FromDays(1);
        updateTimer.Tick += (_, _) => _ = CheckForUpdate();
        updateTimer.Start();
        _ = CheckForUpdate();
    }

    /// <summary>At launch and once a day: a newer release shows in the main window.</summary>
    private async Task CheckForUpdate()
    {
        if (!Updates.Automatic) return;
        try
        {
            if (await Updates.CheckAsync() is { } release) window?.ShowUpdate(release);
        }
        catch (Exception error) when (error is HttpRequestException or TaskCanceledException or System.Text.Json.JsonException or KeyNotFoundException)
        {
            // Offline, or nothing published yet: the next check tries again.
        }
    }

    /// <summary>
    /// Lets installed browsers start opendm-host.exe, which connects the
    /// extension to this app. Rewritten at each launch in case the app moved.
    /// </summary>
    private static void InstallBrowserHost(DownloadCore core)
    {
        var host = Path.Combine(AppContext.BaseDirectory, "opendm-host.exe");
        if (!File.Exists(host)) return;
        Task.Run(() =>
        {
            try
            {
                core.InstallBrowserHost(host);
            }
            catch (Exception error)
            {
                System.Diagnostics.Debug.WriteLine($"OpenDM: could not register the browser host: {error.Message}");
            }
        });
    }

    /// <summary>Called when another launch redirects to this instance.</summary>
    internal static void ShowMainWindowFromAnyThread()
    {
        current?.dispatcher?.TryEnqueue(() => current.ShowMainWindow());
    }

    private void ShowMainWindow() => window?.ShowAndActivate();

    private void Quit()
    {
        core?.Shutdown();
        tray?.Dispose();
        window?.CloseForExit();
        Exit();
    }

    /// <summary>OPENDM_DATA_DIR and OPENDM_ENGINE override the paths for development.</summary>
    private static CoreConfig Config()
    {
        var dataDir = Environment.GetEnvironmentVariable("OPENDM_DATA_DIR")
            ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "OpenDM");
        var engine = Environment.GetEnvironmentVariable("OPENDM_ENGINE")
            ?? Path.Combine(AppContext.BaseDirectory, "aria2-next.exe");
        return new CoreConfig(engine, dataDir, Native.DownloadsFolder());
    }
}
