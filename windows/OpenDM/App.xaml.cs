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
        window = new MainWindow(store, core);
        tray = new TrayIcon(store, core, ShowMainWindow, Quit);
        core.Start();
        window.ShowAndActivate();
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
