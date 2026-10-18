using System.Collections.ObjectModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Windows.Storage.Pickers;
using WinRT.Interop;

namespace OpenDM;

/// <summary>
/// Where new downloads go before they start: the Download File Info dialog
/// for one, a list for several, or straight to the engine when the user
/// turned confirmation off. A captured download may already be running
/// (TaskId): the core started it to hear the server's first answer.
/// </summary>
internal static class NewDownloads
{
    /// <summary>A download of just an address. Headers must not be null on the way to the core.</summary>
    public static NewDownload Link(string url) => new(url, Headers: []);

    public static void Present(IReadOnlyList<NewDownload> downloads, DownloadStore store, DownloadCore core)
    {
        if (downloads.Count == 0) return;
        if (!core.Settings().ConfirmDownloads)
        {
            var ids = downloads.Where(download => download.TaskId is not null).Select(download => download.TaskId!).ToList();
            var waiting = downloads.Where(download => download.TaskId is null).ToArray();
            if (waiting.Length > 0) ids.AddRange(core.AddDownloads(waiting, false));
            ProgressWindow.Show(ids, store, core);
            return;
        }
        Window window = downloads.Count == 1
            ? new DownloadInfoWindow(downloads[0], store, core)
            : new BatchDownloadWindow(downloads, store, core);
        // Above the browser the download came from; when Windows will not
        // give it the focus, its taskbar button flashes.
        ToolWindows.Show(window, activate: true);
    }

    /// <summary>Asks for a folder in a picker owned by <paramref name="owner"/>.</summary>
    public static async Task<string?> ChooseFolder(Window owner)
    {
        var picker = new FolderPicker { SuggestedStartLocation = PickerLocationId.Downloads };
        picker.FileTypeFilter.Add("*");
        InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(owner));
        return (await picker.PickSingleFolderAsync())?.Path;
    }
}

/// <summary>IDM's Download File Info: confirms one new download before it starts.</summary>
public sealed partial class DownloadInfoWindow : Window
{
    private const double ClientWidth = 540;
    private enum Answer { Start, Later, Cancel }

    private readonly NewDownload download;
    private readonly DownloadStore store;
    private readonly DownloadCore core;
    private readonly ObservableCollection<string> categories;
    /// <summary>
    /// The download, already running while the dialog asks about it, as IDM
    /// does: the server is asked once. Null for streams and magnet links,
    /// which start only when confirmed.
    /// </summary>
    private readonly string? taskId;
    private string folder;
    private ulong? size;
    private bool checking;
    private string? problem;
    // What the user changed stays as they set it when the server answers.
    private string shownName;
    private string shownCategory;
    private bool nameEdited;
    private bool categoryEdited;
    private bool folderChosen;
    private bool answered;

    internal DownloadInfoWindow(NewDownload download, DownloadStore store, DownloadCore core)
    {
        this.download = download;
        this.store = store;
        this.core = core;
        InitializeComponent();
        var plan = core.PlanDownload(download);
        categories = new(core.Settings().Categories.Select(rule => rule.Name).Append("Other"));
        if (!categories.Contains(plan.Category)) categories.Add(plan.Category);
        shownName = plan.Name;
        shownCategory = plan.Category;
        NameBox.Text = plan.Name;
        CategoryBox.ItemsSource = categories;
        CategoryBox.SelectedItem = plan.Category;
        folder = plan.Dir;
        size = plan.Size;
        if (download.MediaQuality is { } quality)
        {
            QualityLabel.Visibility = Visibility.Visible;
            QualityText.Visibility = Visibility.Visible;
            QualityText.Text = quality;
        }
        AddressText.Text = download.Url;
        ToolTipService.SetToolTip(AddressText, download.Url);

        var address = download.Url.ToLowerInvariant();
        if (download.TaskId is { } running)
        {
            taskId = running;
        }
        else if (download.MediaKind is null && new[] { "http://", "https://", "ftp://" }.Any(address.StartsWith))
        {
            taskId = core.AddDownloads([download], false).FirstOrDefault();
        }
        checking = taskId is not null;

        ToolWindows.Setup(this, ClientWidth, topmost: true);
        ToolWindows.Keys(this, enter: () => Finish(Answer.Start), escape: () => Finish(Answer.Cancel));
        Closed += OnClosed;
        if (taskId is not null)
        {
            store.RowsChanged += OnRowsChanged;
            if (store.Row(taskId) is { } row) Show(row);
        }
        Refresh();
    }

    private string Category => CategoryBox.SelectedItem as string ?? shownCategory;

    private void OnRowsChanged(IReadOnlyList<RowChange> changes)
    {
        var change = changes.LastOrDefault(change => change.Item.Id == taskId);
        if (change.Item is not null) Show(change.Item.Row);
    }

    /// <summary>What the running download has learned: the server's name and size, or why it failed.</summary>
    private void Show(TaskRow row)
    {
        if (!nameEdited && NameBox.Text != row.Name)
        {
            shownName = row.Name;
            NameBox.Text = row.Name;
        }
        if (!categoryEdited && !folderChosen && row.Category != Category)
        {
            if (!categories.Contains(row.Category)) categories.Add(row.Category);
            shownCategory = row.Category;
            CategoryBox.SelectedItem = row.Category;
            folder = core.CategoryDir(row.Category);
        }
        if (row.TotalBytes > 0) size = row.TotalBytes;
        problem = row.Status == TaskStatus.Failed ? row.Error ?? "The download failed" : null;
        checking = row.Status is not (TaskStatus.Failed or TaskStatus.Complete) && row.TotalBytes == 0 && row.DoneBytes == 0;
        Refresh();
    }

    private void Refresh()
    {
        Heading.Text = NameBox.Text;
        FileGlyph.Glyph = Format.Glyph(Category);
        Checking.IsActive = checking;
        Checking.Visibility = checking ? Visibility.Visible : Visibility.Collapsed;
        Summary.Text = checking ? "Checking\u2026"
            : problem ?? (size is > 0 ? Format.Bytes(size.Value)
            : download.MediaKind is null ? "Size unknown" : "Size known once downloaded");
        Summary.Foreground = (Brush)Application.Current.Resources[problem is null ? "TextFillColorSecondaryBrush" : "SystemFillColorCriticalBrush"];
        FolderText.Text = folder;
        ToolTipService.SetToolTip(FolderText, folder);
        var remember = folderChosen && Category != "Other";
        if ((RememberBox.Visibility == Visibility.Visible) != remember)
        {
            RememberBox.Visibility = remember ? Visibility.Visible : Visibility.Collapsed;
            DispatcherQueue.TryEnqueue(() => ToolWindows.Fit(this, ClientWidth));
        }
        RememberBox.Content = $"Always save {Category} here";
    }

    private void NameBox_TextChanged(object sender, TextChangedEventArgs e)
    {
        // TextChanged also follows text set from the row, and arrives later.
        if (NameBox.Text != shownName) nameEdited = true;
        Refresh();
    }

    private void CategoryBox_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (Category == shownCategory) return;
        categoryEdited = true;
        shownCategory = Category;
        if (!folderChosen) folder = core.CategoryDir(Category);
        Refresh();
    }

    private async void Browse_Click(object sender, RoutedEventArgs e)
    {
        if (await NewDownloads.ChooseFolder(this) is not { } chosen) return;
        folder = chosen;
        folderChosen = true;
        Refresh();
    }

    private void Start_Click(object sender, RoutedEventArgs e) => Finish(Answer.Start);
    private void Later_Click(object sender, RoutedEventArgs e) => Finish(Answer.Later);
    private void Cancel_Click(object sender, RoutedEventArgs e) => Finish(Answer.Cancel);

    private void Finish(Answer answer)
    {
        if (answered) return;
        answered = true;
        if (answer != Answer.Cancel) SaveFolderForCategory();
        if (taskId is not null)
        {
            switch (answer)
            {
                case Answer.Cancel:
                    core.Remove([taskId], true);
                    break;
                case Answer.Later:
                    core.Pause([taskId]);
                    ApplyDestination();
                    break;
                case Answer.Start:
                    ApplyDestination();
                    if (problem is not null) core.Resume([taskId]);
                    ProgressWindow.Show([taskId], store, core);
                    break;
            }
        }
        else if (answer != Answer.Cancel)
        {
            var ids = core.AddDownloads([Confirmed()], answer == Answer.Later);
            if (answer == Answer.Start) ProgressWindow.Show(ids, store, core);
        }
        Close();
    }

    /// <summary>Sends the running download to the name and folder now in the dialog; it moves there when it completes.</summary>
    private void ApplyDestination()
    {
        if (taskId is null || store.Row(taskId) is not { } row) return;
        var typed = NameBox.Text.Trim();
        var name = nameEdited && typed.Length > 0 && typed != row.Name ? typed : null;
        var dir = folder != row.Dir ? folder : null;
        if (name is not null || dir is not null) core.SetDestination(taskId, dir, name);
    }

    /// <summary>The download as the dialog now describes it.</summary>
    private NewDownload Confirmed()
    {
        var confirmed = download with { Category = Category, Dir = folder };
        return nameEdited ? confirmed with { Filename = NameBox.Text.Trim(), FilenameSource = "user" } : confirmed;
    }

    private void SaveFolderForCategory()
    {
        if (RememberBox.Visibility != Visibility.Visible || RememberBox.IsChecked != true) return;
        var settings = core.Settings();
        var rules = settings.Categories.Select(rule => rule.Name == Category ? rule with { Folder = folder } : rule).ToArray();
        try
        {
            core.SetSettings(settings with { Categories = rules });
        }
        catch (CoreException)
        {
            // The download still goes to the chosen folder.
        }
    }

    /// <summary>The close button is Cancel.</summary>
    private void OnClosed(object sender, WindowEventArgs args)
    {
        store.RowsChanged -= OnRowsChanged;
        if (!answered && taskId is not null)
        {
            answered = true;
            core.Remove([taskId], true);
        }
    }
}
