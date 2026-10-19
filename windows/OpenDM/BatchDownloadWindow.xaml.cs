using System.ComponentModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace OpenDM;

/// <summary>One file in the several-downloads dialog.</summary>
public sealed partial class BatchItem : INotifyPropertyChanged
{
    private bool? chosen = true;

    internal BatchItem(NewDownload download, string name, string category)
    {
        Download = download;
        Name = name;
        Category = category;
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    internal NewDownload Download { get; }
    public string Name { get; }

    /// <summary>What screen readers say for the list item.</summary>
    public override string ToString() => Name;
    public string Category { get; }
    public string Url => Download.Url;
    public string Glyph => Format.Glyph(Category);

    public bool? Chosen
    {
        get => chosen;
        set
        {
            if (chosen == value) return;
            chosen = value;
            PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(Chosen)));
        }
    }
}

/// <summary>Confirms several downloads at once, such as all links on a page.</summary>
public sealed partial class BatchDownloadWindow : Window
{
    private const double ClientWidth = 620;

    private readonly DownloadStore store;
    private readonly DownloadCore core;
    private readonly List<BatchItem> items;
    /// <summary>Null: each file goes to its category's folder.</summary>
    private string? folder;
    private bool answered;

    internal BatchDownloadWindow(IReadOnlyList<NewDownload> downloads, DownloadStore store, DownloadCore core)
    {
        this.store = store;
        this.core = core;
        InitializeComponent();
        Title = $"Download {downloads.Count} Files";
        items = downloads.Select(download =>
        {
            var plan = core.PlanDownload(download);
            var item = new BatchItem(download, plan.Name, plan.Category);
            item.PropertyChanged += (_, _) => Refresh();
            return item;
        }).ToList();
        Files.ItemsSource = items;
        ToolWindows.Setup(this, ClientWidth, topmost: true);
        ToolWindows.Keys(this, enter: () => { if (Chosen().Count > 0) Finish(start: true); }, escape: Close);
        Refresh();
    }

    private List<BatchItem> Chosen() => items.Where(item => item.Chosen == true).ToList();

    private void Refresh()
    {
        var chosen = Chosen().Count;
        CountText.Text = $"{chosen} of {items.Count} {(items.Count == 1 ? "file" : "files")} selected";
        AllButton.IsEnabled = chosen < items.Count;
        NoneButton.IsEnabled = chosen > 0;
        StartButton.Content = chosen > 1 ? "Start Downloads" : "Start Download";
        StartButton.IsEnabled = chosen > 0;
        LaterButton.IsEnabled = chosen > 0;
        FolderText.Text = folder ?? "Each file\u2019s category folder";
        ToolTipService.SetToolTip(FolderText, folder);
        CategoryFoldersButton.Visibility = folder is null ? Visibility.Collapsed : Visibility.Visible;
    }

    private void SetAll(bool on)
    {
        foreach (var item in items) item.Chosen = on;
    }

    private void All_Click(object sender, RoutedEventArgs e) => SetAll(true);
    private void None_Click(object sender, RoutedEventArgs e) => SetAll(false);

    private async void Browse_Click(object sender, RoutedEventArgs e)
    {
        if (await NewDownloads.ChooseFolder(this) is not { } chosen) return;
        folder = chosen;
        Refresh();
    }

    private void CategoryFolders_Click(object sender, RoutedEventArgs e)
    {
        folder = null;
        Refresh();
    }

    private void Start_Click(object sender, RoutedEventArgs e) => Finish(start: true);
    private void Later_Click(object sender, RoutedEventArgs e) => Finish(start: false);
    private void Cancel_Click(object sender, RoutedEventArgs e) => Close();

    private void Finish(bool start)
    {
        if (answered) return;
        answered = true;
        var downloads = Chosen().Select(item => folder is null ? item.Download : item.Download with { Dir = folder }).ToArray();
        var ids = core.AddDownloads(downloads, startPaused: !start);
        if (start) ProgressWindow.Show(ids, store, core);
        Close();
    }
}
