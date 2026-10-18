using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;

namespace OpenDM;

public partial class App : Application
{
    private static App? current;
    private DispatcherQueue? dispatcher;
    private DownloadCore? core;
    private MainWindow? window;
    private TrayIcon? tray;

    public App()
    {
        current = this;
        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        dispatcher = DispatcherQueue.GetForCurrentThread();
        var store = new DownloadStore(dispatcher);
        try
        {
            core = new DownloadCore(Config(), store.Listener);
        }
        catch (Exception error)
        {
            Native.ShowError("OpenDM could not open its data", error.Message);
            Exit();
            return;
        }
        store.Load(core);
        store.BrowserDownloadsReceived += (downloads, _) =>
        {
            // Until the Download File Info dialog exists, start right away
            // and show the list so the handoff is visible.
            core.AddDownloads(downloads, false);
            ShowMainWindow();
        };
        window = new MainWindow(store, core);
        tray = new TrayIcon(store, core, ShowMainWindow, Quit);
        core.Start();
        InstallBrowserHost(core);
        // The browser extension starts OpenDM with --background when it is not running.
        if (!Environment.GetCommandLineArgs().Contains("--background"))
        {
            window.ShowAndActivate();
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
