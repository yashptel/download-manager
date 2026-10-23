using Microsoft.UI.Xaml;
using Windows.System;

namespace OpenDM;

/// <summary>
/// IDM's "Refresh download address": an expired download gets a new
/// address from the browser. The page it came from opens, the user starts
/// the same download there again, and the next download the browser hands
/// over is offered as the new address. The old download then continues from
/// the part already on disk.
/// </summary>
internal static class AddressRefresh
{
    private static AddressWindow? window;

    /// <exception cref="CoreException.Failed">The download cannot get a new address; the message says why.</exception>
    public static void Start(TaskRow row, DownloadStore store, DownloadCore core)
    {
        var page = core.WaitForAddress(row.Id);
        // A new wait replaces the last one in the core.
        window?.Dismiss();
        Open(new AddressWindow(row, page, store, core));
        if (page is not null && Uri.TryCreate(page, UriKind.Absolute, out var uri)) _ = Launcher.LaunchUriAsync(uri);
    }

    /// <summary>The browser handed over a download while OpenDM was waiting.</summary>
    public static void Received(string id, NewDownload download, DownloadStore store, DownloadCore core)
    {
        if (store.Row(id) is not { } row)
        {
            window?.Dismiss();
            NewDownloads.Present([download], store, core);
            return;
        }
        var asking = window ?? Open(new AddressWindow(row, null, store, core));
        asking.Ask(download);
        asking.Activate();
    }

    private static AddressWindow Open(AddressWindow opened)
    {
        window = opened;
        opened.Closed += (_, _) => { if (window == opened) window = null; };
        ToolWindows.Show(opened);
        return opened;
    }
}

/// <summary>Waits for the new address, then asks whether the download the browser sent is it.</summary>
public sealed partial class AddressWindow : Window
{
    private const double ClientWidth = 480;

    private readonly TaskRow row;
    private readonly DownloadStore store;
    private readonly DownloadCore core;
    private NewDownload? offered;
    private bool done;

    internal AddressWindow(TaskRow row, string? page, DownloadStore store, DownloadCore core)
    {
        this.row = row;
        this.store = store;
        this.core = core;
        InitializeComponent();
        FileGlyph.Glyph = Format.Glyph(row.Category);
        Heading.Text = $"Waiting for a new address for \u201c{row.Name}\u201d";
        var progress = row.DoneBytes == 0 ? "from the start"
            : $"from {Format.Bytes(row.DoneBytes)}" + (row.TotalBytes > 0 ? $" of {Format.Bytes(row.TotalBytes)}" : "");
        Body.Text = page is null
            ? $"Start the same download again in your browser. OpenDM takes its new address and continues {progress}."
            : $"The page it came from is open in your browser. Start the same download there again, and OpenDM takes its new address and continues {progress}.";
        // It waits beside the browser, which needs the screen now.
        ToolWindows.Setup(this, ClientWidth);
        ToolWindows.Keys(this, enter: () => { if (offered is not null) Finish(use: true); }, escape: Close);
        Closed += (_, _) => OnClosed();
    }

    /// <summary>Shows the download the browser sent and asks whether it is the new address.</summary>
    internal void Ask(NewDownload download)
    {
        offered = download;
        Waiting.IsActive = false;
        Waiting.Visibility = Visibility.Collapsed;
        var name = core.PlanDownload(download).Name;
        Heading.Text = $"Is this the new address for \u201c{row.Name}\u201d?";
        Body.Text = "New: " + name + (download.Size is { } size ? $" ({Format.Bytes(size)})" : "");
        Address.Text = download.Url;
        Address.Visibility = Visibility.Visible;
        var total = store.Row(row.Id)?.TotalBytes ?? row.TotalBytes;
        if (download.Size is > 0 && total > 0 && download.Size != total)
        {
            Warning.Text = $"Its size differs from the download\u2019s ({Format.Bytes(total)}): it may be another file.";
            Warning.Visibility = Visibility.Visible;
        }
        UseButton.Visibility = Visibility.Visible;
        OtherButton.Content = "Download It Separately";
        ToolWindows.Fit(this, ClientWidth);
    }

    /// <summary>Closes without telling the core: a new wait has replaced this one.</summary>
    internal void Dismiss()
    {
        done = true;
        Close();
    }

    private void Use_Click(object sender, RoutedEventArgs e) => Finish(use: true);
    private void Other_Click(object sender, RoutedEventArgs e) => Close();

    private void Finish(bool use)
    {
        if (done || offered is null) return;
        done = true;
        if (use)
        {
            core.RefreshAddress(row.Id, offered);
            ProgressWindow.Show([row.Id], store, core);
        }
        else
        {
            NewDownloads.Present([offered], store, core);
        }
        Close();
    }

    /// <summary>The close button is Cancel while waiting, and Download It Separately once a download was offered.</summary>
    private void OnClosed()
    {
        if (done) return;
        if (offered is not null)
        {
            Finish(use: false);
            return;
        }
        done = true;
        core.StopWaitingForAddress();
    }
}

