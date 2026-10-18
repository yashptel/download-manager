using System.Diagnostics;
using Microsoft.UI.Input;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.ApplicationModel.DataTransfer;
using Windows.System;
using Windows.UI.Core;
using WinRT.Interop;

namespace OpenDM;

internal abstract record ListFilter
{
    public static readonly ListFilter All = new AllDownloads();
    public static readonly ListFilter Unfinished = new UnfinishedDownloads();
    public static readonly ListFilter Finished = new FinishedDownloads();

    public sealed record AllDownloads : ListFilter;
    public sealed record UnfinishedDownloads : ListFilter;
    public sealed record FinishedDownloads : ListFilter;
    public sealed record Category(string Name) : ListFilter;

    public bool Matches(TaskRow row) => this switch
    {
        UnfinishedDownloads => row.Status != TaskStatus.Complete,
        FinishedDownloads => row.Status == TaskStatus.Complete,
        Category category => row.Category == category.Name,
        _ => true,
    };
}

internal enum SortColumn { Name, Size, Progress, Speed, Eta, Status }

/// <summary>
/// Main window: category sidebar, toolbar and the download list. Progress
/// updates go straight to row items; only filter and sort changes touch the
/// list itself. Closing the window hides it; the app keeps running in the
/// notification area.
/// </summary>
public sealed partial class MainWindow : Window
{
    private readonly DownloadStore store;
    private readonly DownloadCore core;
    private readonly DownloadList visible = new();
    private readonly Dictionary<ListFilter, NavigationViewItem> navItems = new();
    private readonly Dictionary<SortColumn, (Button Button, string Title)> headers;
    private HashSet<string> shownIds = new();
    private ListFilter filter = ListFilter.All;
    private string search = "";
    private SortColumn? sortColumn;
    private bool sortAscending = true;
    private bool dialogOpen;
    private bool engineFailureShown;
    private bool exiting;

    internal MainWindow(DownloadStore store, DownloadCore core)
    {
        this.store = store;
        this.core = core;
        InitializeComponent();

        ExtendsContentIntoTitleBar = true;
        SetTitleBar(TitleBar);
        // Caption buttons follow the light/dark app mode, also when it changes while running.
        AppWindow.TitleBar.PreferredTheme = TitleBarTheme.UseDefaultAppMode;
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "OpenDM.ico"));
        Root.Loaded += (_, _) =>
        {
            ShowAppIcon();
            Content.XamlRoot.Changed += (_, _) => ShowAppIcon();
        };
        CenterOnScreen(1120, 700);
        AppWindow.Closing += AppWindow_Closing;
        AppWindow.Changed += AppWindow_Changed;

        headers = new()
        {
            [SortColumn.Name] = (NameHeader, "Name"),
            [SortColumn.Size] = (SizeHeader, "Size"),
            [SortColumn.Progress] = (ProgressHeader, "Progress"),
            [SortColumn.Speed] = (SpeedHeader, "Speed"),
            [SortColumn.Eta] = (EtaHeader, "Time left"),
            [SortColumn.Status] = (StatusHeader, "Status"),
        };
        BuildNavigation(core.Settings().Categories.Select(rule => rule.Name).Append("Other"));
        Table.ItemsSource = visible;
        store.LayoutRoot = Root;

        store.RowsChanged += OnRowsChanged;
        store.RowsRemoved += _ =>
        {
            Rebuild();
            Recount();
            UpdateCommands();
        };
        store.StatusChanged += OnStatusChanged;
        store.ErrorRaised += message => _ = ShowMessage(message);

        Rebuild();
        Recount();
        OnStatusChanged();
    }

    internal void ShowAndActivate()
    {
        if (AppWindow.Presenter is OverlappedPresenter { State: OverlappedPresenterState.Minimized } presenter)
        {
            presenter.Restore();
        }
        AppWindow.Show();
        Activate();
        Native.SetForegroundWindow(WindowNative.GetWindowHandle(this));
        core.SetWindowVisible(true);
    }

    internal void CloseForExit()
    {
        exiting = true;
        Close();
    }

    /// <summary>The title bar icon drawn for the screen's scale, so it stays crisp.</summary>
    private void ShowAppIcon()
    {
        var pixels = 16 * Content.XamlRoot.RasterizationScale;
        var size = pixels <= 16 ? 16 : pixels <= 24 ? 24 : 32;
        var path = Path.Combine(AppContext.BaseDirectory, "Assets", $"OpenDM-{size}.png");
        if (AppIcon.Source is not BitmapImage { UriSource.LocalPath: var current } || current != path)
        {
            AppIcon.Source = new BitmapImage(new Uri(path));
        }
    }

    private void CenterOnScreen(int width, int height)
    {
        var area = DisplayArea.GetFromWindowId(AppWindow.Id, DisplayAreaFallback.Primary).WorkArea;
        // XamlRoot does not exist yet, so take the scale from the window's DPI.
        var dpi = Native.GetDpiForWindow(WindowNative.GetWindowHandle(this));
        var scale = dpi > 0 ? dpi / 96.0 : 1.0;
        var size = new Windows.Graphics.SizeInt32((int)(width * scale), (int)(height * scale));
        size.Width = Math.Min(size.Width, area.Width);
        size.Height = Math.Min(size.Height, area.Height);
        AppWindow.MoveAndResize(new Windows.Graphics.RectInt32(
            area.X + (area.Width - size.Width) / 2,
            area.Y + (area.Height - size.Height) / 2,
            size.Width,
            size.Height));
    }

    private void AppWindow_Closing(AppWindow sender, AppWindowClosingEventArgs args)
    {
        if (exiting) return;
        args.Cancel = true;
        sender.Hide();
        core.SetWindowVisible(false);
    }

    private void AppWindow_Changed(AppWindow sender, AppWindowChangedEventArgs args)
    {
        if (!args.DidVisibilityChange && !args.DidPresenterChange && !args.DidSizeChange) return;
        var minimized = sender.Presenter is OverlappedPresenter { State: OverlappedPresenterState.Minimized };
        core.SetWindowVisible(sender.IsVisible && !minimized);
    }

    // Sidebar

    private void BuildNavigation(IEnumerable<string> categories)
    {
        AddNavItem(ListFilter.All, "All downloads", "\uE8FD");
        AddNavItem(ListFilter.Unfinished, "Unfinished", "\uE896");
        AddNavItem(ListFilter.Finished, "Finished", "\uE73E");
        Nav.MenuItems.Add(new NavigationViewItemHeader { Content = "Categories" });
        foreach (var name in categories)
        {
            AddNavItem(new ListFilter.Category(name), name, Format.Glyph(name));
        }
        Nav.SelectedItem = navItems[ListFilter.All];
    }

    private void AddNavItem(ListFilter itemFilter, string title, string glyph)
    {
        var item = new NavigationViewItem
        {
            Content = title,
            Icon = new FontIcon { Glyph = glyph },
            Tag = itemFilter,
            // Down 1 px, to the middle of the label's capitals.
            InfoBadge = new InfoBadge { Visibility = Visibility.Collapsed, Margin = new Thickness(0, 2, 0, 0) },
        };
        navItems[itemFilter] = item;
        Nav.MenuItems.Add(item);
    }

    private void Nav_SelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (args.SelectedItem is NavigationViewItem { Tag: ListFilter selected } && selected != filter)
        {
            filter = selected;
            Rebuild();
        }
    }

    private void Recount()
    {
        var counts = new Dictionary<ListFilter, int>();
        void Count(ListFilter key) => counts[key] = counts.GetValueOrDefault(key) + 1;
        foreach (var item in store.Items)
        {
            Count(ListFilter.All);
            Count(item.Row.Status == TaskStatus.Complete ? ListFilter.Finished : ListFilter.Unfinished);
            Count(new ListFilter.Category(item.Row.Category));
        }
        foreach (var (key, nav) in navItems)
        {
            var count = counts.GetValueOrDefault(key);
            nav.InfoBadge.Value = count;
            nav.InfoBadge.Visibility = count > 0 ? Visibility.Visible : Visibility.Collapsed;
        }
    }

    // List contents

    private void OnRowsChanged(IReadOnlyList<RowChange> changes)
    {
        var structural = false;
        var recount = false;
        var statusChanged = false;
        foreach (var (old, item) in changes)
        {
            var row = item.Row;
            if (old is null
                || (old.Status == TaskStatus.Complete) != (row.Status == TaskStatus.Complete)
                || old.Category != row.Category)
            {
                recount = true;
            }
            statusChanged |= old is null || old.Status != row.Status;
            structural |= shownIds.Contains(row.Id) != Matches(row)
                || (old is not null && sortColumn is { } column
                    && Compare(SortKey(old, column), SortKey(row, column), column) != 0);
        }
        if (structural) Rebuild();
        if (recount) Recount();
        if (statusChanged) UpdateCommands();
    }

    private bool Matches(TaskRow row) =>
        filter.Matches(row)
        && (search.Length == 0
            || row.Name.Contains(search, StringComparison.CurrentCultureIgnoreCase)
            || row.Url.Contains(search, StringComparison.OrdinalIgnoreCase));

    /// <summary>What a column sorts by; speed as displayed, so equal-looking speeds tie.</summary>
    private static (double Number, string? Text) SortKey(TaskRow row, SortColumn column) => column switch
    {
        SortColumn.Name => (0, row.Name),
        SortColumn.Size => (row.TotalBytes, null),
        SortColumn.Progress => (row.Progress, null),
        SortColumn.Speed => (Format.Displayed(row.Speed), null),
        SortColumn.Eta => (row.EtaSecs ?? double.MaxValue, null),
        _ => (0, Format.Status(row)),
    };

    private static int Compare((double Number, string? Text) a, (double Number, string? Text) b, SortColumn column) => column switch
    {
        SortColumn.Name => Native.CompareNatural(a.Text!, b.Text!),
        SortColumn.Status => string.Compare(a.Text, b.Text, StringComparison.CurrentCulture),
        _ => a.Number.CompareTo(b.Number),
    };

    private void Rebuild()
    {
        var selected = SelectedIds();
        var next = store.Items.Where(item => Matches(item.Row)).ToList();
        if (sortColumn is { } column)
        {
            var sign = sortAscending ? 1 : -1;
            // Rows that compare equal keep their current order, so live values that
            // jitter around the same displayed number do not swap rows back and forth.
            var shown = new Dictionary<DownloadItem, int>(visible.Count, ReferenceEqualityComparer.Instance);
            for (var i = 0; i < visible.Count; i++)
            {
                shown[visible[i]] = i;
            }
            var keyed = next
                .Select(item => (Item: item, Key: SortKey(item.Row, column), Shown: shown.GetValueOrDefault(item, int.MaxValue)))
                .ToArray();
            Array.Sort(keyed, (a, b) =>
            {
                var order = Compare(a.Key, b.Key, column) * sign;
                if (order != 0) return order;
                order = a.Shown.CompareTo(b.Shown);
                if (order != 0) return order;
                order = a.Item.Row.AddedAt.CompareTo(b.Item.Row.AddedAt);
                return order != 0 ? order : string.CompareOrdinal(a.Item.Id, b.Item.Id);
            });
            next = keyed.Select(entry => entry.Item).ToList();
        }
        shownIds = next.Select(item => item.Id).ToHashSet();
        foreach (var item in visible.ApplyOrder(next))
        {
            if (selected.Contains(item.Id)) Table.SelectedItems.Add(item);
        }
        EmptyText.Text = store.Items.Count == 0
            ? "Drop a link here or press Ctrl+N to add a download."
            : "No downloads here.";
        EmptyText.Visibility = next.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    private void Header_Click(object sender, RoutedEventArgs e)
    {
        var column = Enum.Parse<SortColumn>((string)((Button)sender).Tag);
        if (sortColumn == column)
        {
            sortAscending = !sortAscending;
        }
        else
        {
            sortColumn = column;
            sortAscending = column == SortColumn.Name;
        }
        foreach (var (key, (button, title)) in headers)
        {
            button.Content = key == column ? SortedHeader(title, sortAscending) : title;
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(button,
                key == column ? $"{title}, sorted {(sortAscending ? "ascending" : "descending")}" : title);
        }
        Rebuild();
    }

    /// <summary>
    /// A header title followed by an up or down arrow from Segoe Fluent Icons,
    /// as tall as the capitals and centered on them.
    /// </summary>
    private static StackPanel SortedHeader(string title, bool ascending) => new()
    {
        Orientation = Orientation.Horizontal,
        Spacing = 6,
        Children =
        {
            new TextBlock { Text = title },
            new FontIcon
            {
                Glyph = ascending ? "\uE74A" : "\uE74B",
                FontSize = 10,
                FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
                VerticalAlignment = VerticalAlignment.Center,
                Margin = new Thickness(0, 2, 0, 0),
            },
        },
    };

    private void Search_TextChanged(AutoSuggestBox sender, AutoSuggestBoxTextChangedEventArgs args)
    {
        search = sender.Text.Trim();
        Rebuild();
    }

    private void FocusSearch_Invoked(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        SearchBox.Focus(FocusState.Keyboard);
        args.Handled = true;
    }

    // Selection and commands

    private List<TaskRow> SelectedRows() => Table.SelectedItems.OfType<DownloadItem>().Select(item => item.Row).ToList();

    private HashSet<string> SelectedIds() => Table.SelectedItems.OfType<DownloadItem>().Select(item => item.Id).ToHashSet();

    private static bool CanResume(TaskRow row) => row.Status is TaskStatus.Paused or TaskStatus.Failed;

    private static bool CanPause(TaskRow row) => row.Status is TaskStatus.Downloading or TaskStatus.Queued;

    private void UpdateCommands()
    {
        var rows = SelectedRows();
        ResumeButton.IsEnabled = rows.Any(CanResume);
        PauseButton.IsEnabled = rows.Any(CanPause);
        RemoveButton.IsEnabled = rows.Count > 0;
    }

    private void Table_SelectionChanged(object sender, SelectionChangedEventArgs e) => UpdateCommands();

    private void Resume(IReadOnlyList<TaskRow> rows)
    {
        var ids = rows.Where(CanResume).Select(row => row.Id).ToArray();
        if (ids.Length == 0) return;
        core.Resume(ids);
        ProgressWindow.Show(ids, store, core);
    }

    private void Pause(IReadOnlyList<TaskRow> rows)
    {
        var ids = rows.Where(CanPause).Select(row => row.Id).ToArray();
        if (ids.Length > 0) core.Pause(ids);
    }

    private async Task Remove(IReadOnlyList<TaskRow> rows, bool deleteFiles)
    {
        if (rows.Count == 0) return;
        if (deleteFiles)
        {
            var question = rows.Count == 1
                ? $"Delete \u201C{rows[0].Name}\u201D from disk?"
                : $"Delete the files of {rows.Count} downloads from disk?";
            var dialog = new ContentDialog
            {
                Title = question,
                Content = "The files are deleted permanently.",
                PrimaryButtonText = "Delete",
                CloseButtonText = "Cancel",
                DefaultButton = ContentDialogButton.Close,
            };
            if (await ShowDialog(dialog) != ContentDialogResult.Primary) return;
        }
        core.Remove(rows.Select(row => row.Id).ToArray(), deleteFiles);
    }

    private static void Open(IEnumerable<TaskRow> rows)
    {
        foreach (var row in rows.Where(row => row.Status == TaskStatus.Complete && row.Path is not null))
        {
            Shell.Open(row.Path!);
        }
    }

    private static void Reveal(IReadOnlyList<TaskRow> rows)
    {
        if (rows.Count > 0) Shell.Reveal(rows[0].Path, rows[0].Dir);
    }

    private static void CopyAddresses(IReadOnlyList<TaskRow> rows)
    {
        if (rows.Count == 0) return;
        var package = new DataPackage();
        package.SetText(string.Join("\r\n", rows.Select(row => row.Url)));
        Clipboard.SetContent(package);
    }

    private void Add_Click(object sender, RoutedEventArgs e) => _ = ShowNewDownload();
    private void Resume_Click(object sender, RoutedEventArgs e) => Resume(SelectedRows());
    private void Pause_Click(object sender, RoutedEventArgs e) => Pause(SelectedRows());
    private void Remove_Click(object sender, RoutedEventArgs e) => _ = Remove(SelectedRows(), false);
    private void ResumeAll_Click(object sender, RoutedEventArgs e) => core.ResumeAll();
    private void PauseAll_Click(object sender, RoutedEventArgs e) => core.PauseAll();

    private void Table_PreviewKeyDown(object sender, KeyRoutedEventArgs e)
    {
        static bool Down(VirtualKey key) =>
            InputKeyboardSource.GetKeyStateForCurrentThread(key).HasFlag(CoreVirtualKeyStates.Down);
        var control = Down(VirtualKey.Control);
        switch (e.Key)
        {
            case VirtualKey.Delete:
                // Shift+Delete deletes the files too, as in Explorer.
                _ = Remove(SelectedRows(), Down(VirtualKey.Shift));
                break;
            case VirtualKey.Space when !control:
                var rows = SelectedRows();
                if (rows.Any(CanPause)) Pause(rows); else Resume(rows);
                break;
            case VirtualKey.Enter:
                Open(SelectedRows());
                break;
            case VirtualKey.C when control:
                CopyAddresses(SelectedRows());
                break;
            case VirtualKey.V when control:
                _ = ShowNewDownload();
                break;
            default:
                return;
        }
        e.Handled = true;
    }

    private void Table_DoubleTapped(object sender, DoubleTappedRoutedEventArgs e)
    {
        if ((e.OriginalSource as FrameworkElement)?.DataContext is DownloadItem item)
        {
            Open([item.Row]);
        }
    }

    private void Table_ContextRequested(UIElement sender, ContextRequestedEventArgs e)
    {
        var source = e.OriginalSource as FrameworkElement;
        var item = source?.DataContext as DownloadItem ?? (source as ListViewItem)?.Content as DownloadItem;
        if (item is null) return;
        if (!Table.SelectedItems.Contains(item))
        {
            Table.SelectedItems.Clear();
            Table.SelectedItems.Add(item);
        }
        var menu = BuildContextMenu(SelectedRows());
        if (e.TryGetPosition(Table, out var point))
        {
            menu.ShowAt(Table, point);
        }
        else
        {
            menu.ShowAt(source!);
        }
        e.Handled = true;
    }

    private MenuFlyout BuildContextMenu(IReadOnlyList<TaskRow> rows)
    {
        var menu = new MenuFlyout();
        void Add(string text, bool enabled, Action action)
        {
            var entry = new MenuFlyoutItem { Text = text, IsEnabled = enabled };
            entry.Click += (_, _) => action();
            menu.Items.Add(entry);
        }
        Add("Open", rows.Any(row => row.Status == TaskStatus.Complete), () => Open(rows));
        Add("Show in folder", true, () => Reveal(rows));
        menu.Items.Add(new MenuFlyoutSeparator());
        Add("Resume", rows.Any(CanResume), () => Resume(rows));
        Add("Pause", rows.Any(CanPause), () => Pause(rows));
        menu.Items.Add(new MenuFlyoutSeparator());
        Add("Copy address", true, () => CopyAddresses(rows));
        menu.Items.Add(new MenuFlyoutSeparator());
        Add("Remove", true, () => _ = Remove(rows, false));
        Add("Remove and delete files\u2026", true, () => _ = Remove(rows, true));
        return menu;
    }

    // Adding downloads

    private void Content_DragOver(object sender, DragEventArgs e)
    {
        if (e.DataView.Contains(StandardDataFormats.WebLink) || e.DataView.Contains(StandardDataFormats.Text))
        {
            e.AcceptedOperation = DataPackageOperation.Copy;
        }
    }

    private async void Content_Drop(object sender, DragEventArgs e)
    {
        var deferral = e.GetDeferral();
        try
        {
            var text = e.DataView.Contains(StandardDataFormats.WebLink)
                ? (await e.DataView.GetWebLinkAsync()).AbsoluteUri
                : await e.DataView.GetTextAsync();
            var urls = Links.Find(text);
            if (urls.Length > 0) core.AddUrls(urls, false);
        }
        finally
        {
            deferral.Complete();
        }
    }

    private async Task ShowNewDownload()
    {
        if (dialogOpen) return;
        ShowAndActivate();
        var box = new TextBox
        {
            AcceptsReturn = true,
            TextWrapping = TextWrapping.NoWrap,
            Height = 120,
            Width = 460,
            PlaceholderText = "https://example.com/file.zip",
            Text = string.Join(Environment.NewLine, Links.Find(await ClipboardText())),
        };
        ScrollViewer.SetVerticalScrollBarVisibility(box, ScrollBarVisibility.Auto);
        box.SelectAll();
        var panel = new StackPanel { Spacing = 8 };
        panel.Children.Add(new TextBlock { Text = "Enter one or more addresses, one per line." });
        panel.Children.Add(box);
        var dialog = new ContentDialog
        {
            Title = "New download",
            Content = panel,
            PrimaryButtonText = "Start",
            SecondaryButtonText = "Download later",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
        };
        var result = await ShowDialog(dialog);
        var urls = Links.Find(box.Text);
        if (urls.Length == 0) return;
        if (result == ContentDialogResult.Primary) core.AddUrls(urls, false);
        else if (result == ContentDialogResult.Secondary) core.AddUrls(urls, true);
    }

    private static async Task<string> ClipboardText()
    {
        try
        {
            var content = Clipboard.GetContent();
            return content.Contains(StandardDataFormats.Text) ? await content.GetTextAsync() : "";
        }
        catch (Exception)
        {
            // Another app can hold the clipboard; start with an empty box.
            return "";
        }
    }

    // Status and messages

    private void OnStatusChanged()
    {
        var stats = store.Stats;
        Subtitle.Text = store.EngineState switch
        {
            EngineState.Starting => "Starting\u2026",
            EngineState.Restarting => "Restarting the download engine\u2026",
            EngineState.Failed => "The download engine stopped",
            EngineState.Stopped => "",
            _ when stats.Active > 0 && stats.DownloadSpeed > 0 =>
                $"{stats.Active} downloading \u00B7 {Format.Speed(stats.DownloadSpeed)}",
            _ when stats.Active > 0 => $"{stats.Active} downloading",
            _ when stats.Waiting > 0 => $"{stats.Waiting} waiting",
            _ => "",
        };
        switch (store.EngineState)
        {
            case EngineState.Failed failed when !engineFailureShown:
                engineFailureShown = true;
                _ = ShowEngineFailure(failed.Reason);
                break;
            case EngineState.Running:
                engineFailureShown = false;
                break;
        }
    }

    private async Task ShowEngineFailure(string reason)
    {
        ShowAndActivate();
        var dialog = new ContentDialog
        {
            Title = "The download engine keeps stopping",
            Content = reason,
            PrimaryButtonText = "Restart engine",
            CloseButtonText = "Close",
            DefaultButton = ContentDialogButton.Primary,
        };
        if (await ShowDialog(dialog) == ContentDialogResult.Primary)
        {
            core.RestartEngine();
        }
    }

    private async Task ShowMessage(string message)
    {
        if (!AppWindow.IsVisible || dialogOpen)
        {
            Debug.WriteLine($"OpenDM: {message}");
            return;
        }
        await ShowDialog(new ContentDialog { Title = message, CloseButtonText = "OK" });
    }

    /// <summary>Shows one dialog at a time; WinUI allows only one per window.</summary>
    private async Task<ContentDialogResult> ShowDialog(ContentDialog dialog)
    {
        if (dialogOpen) return ContentDialogResult.None;
        dialogOpen = true;
        try
        {
            dialog.XamlRoot = Content.XamlRoot;
            dialog.Style = (Style)Application.Current.Resources["DefaultContentDialogStyle"];
            return await dialog.ShowAsync();
        }
        finally
        {
            dialogOpen = false;
        }
    }
}
