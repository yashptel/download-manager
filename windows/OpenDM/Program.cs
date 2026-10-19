using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.Windows.AppLifecycle;

namespace OpenDM;

public static class Program
{
    /// <summary>
    /// One OpenDM per user and data folder: a second launch hands its
    /// activation to the running instance, which shows its window, and
    /// exits. A copy with its own OPENDM_DATA_DIR runs beside the installed
    /// one, as its browser pipe does.
    /// </summary>
    [STAThread]
    private static int Main(string[] args)
    {
        WinRT.ComWrappersSupport.InitializeComWrappers();
        var instance = AppInstance.FindOrRegisterForKey(InstanceKey());
        if (!instance.IsCurrent)
        {
            var activation = AppInstance.GetCurrent().GetActivatedEventArgs();
            // Wait off the STA thread so COM keeps pumping.
            Task.Run(() => instance.RedirectActivationToAsync(activation).AsTask()).Wait();
            return 0;
        }
        instance.Activated += (_, _) => App.ShowMainWindowFromAnyThread();

        Application.Start(_ =>
        {
            var context = new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread());
            SynchronizationContext.SetSynchronizationContext(context);
            new App();
        });
        return 0;
    }

    private static string InstanceKey()
    {
        var dataDir = Environment.GetEnvironmentVariable("OPENDM_DATA_DIR");
        if (string.IsNullOrEmpty(dataDir)) return "OpenDM.Main";
        var hash = System.Security.Cryptography.SHA256.HashData(System.Text.Encoding.UTF8.GetBytes(Path.GetFullPath(dataDir).ToLowerInvariant()));
        return "OpenDM.Main." + Convert.ToHexString(hash, 0, 8);
    }
}
