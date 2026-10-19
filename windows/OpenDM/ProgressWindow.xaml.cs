using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Microsoft.Win32;

namespace OpenDM;

/// <summary>
/// IDM's download progress window: one small window per download started
/// from the dialog or resumed from the list, with Pause and Cancel, that
/// turns into a "Download Complete" window.
/// </summary>
public sealed partial class ProgressWindow : Window
{
    private const double ClientWidth = 460;
    /// <summary>More downloads than this started at once (a page of links) get no windows.</summary>
    private const int MaxAtOnce = 3;
    private const string DetailsKey = @"Software\OpenDM";
    private static readonly Dictionary<string, ProgressWindow> Open = new();

    /// <summary>Raised when a progress window opens or closes.</summary>
    internal static event Action? OpenChanged;
    internal static bool AnyOpen => Open.Count > 0;

    private readonly string id;
    private readonly DownloadStore store;
    private readonly DownloadCore core;
    private readonly DispatcherQueueTimer timer;
    private readonly List<Rectangle> cells = new();
    private TaskRow? row;
    private TaskDetails? details;
    private bool fetching;
    private bool closed;

    /// <summary>Opens windows for downloads that just started, when the settings want them.</summary>
    internal static void Show(IReadOnlyCollection<string> ids, DownloadStore store, DownloadCore core)
    {
        if (ids.Count == 0 || ids.Count > MaxAtOnce || !core.Settings().ShowProgressWindow) return;
        foreach (var id in ids)
        {
            if (Open.TryGetValue(id, out var existing))
            {
                existing.AppWindow.MoveInZOrderAtTop();
                continue;
            }
            var window = new ProgressWindow(id, store, core);
            Open[id] = window;
            ToolWindows.Show(window, activate: false);
            OpenChanged?.Invoke();
        }
    }

    private ProgressWindow(string id, DownloadStore store, DownloadCore core)
    {
        this.id = id;
        this.store = store;
        this.core = core;
        InitializeComponent();
        ToolWindows.Setup(this, ClientWidth, minimizable: true);
        ToolWindows.Keys(this, enter: () => Primary_Click(this, new RoutedEventArgs()), escape: () =>
        {
            if (row?.Status == TaskStatus.Complete) Close(); else Secondary_Click(this, new RoutedEventArgs());
        });
        DetailsExpander.IsExpanded = Registry.CurrentUser.OpenSubKey(DetailsKey)?.GetValue("ProgressDetails") is 1;
        row = store.Row(id);
        store.RowsChanged += OnRowsChanged;
        store.RowsRemoved += OnRowsRemoved;
        timer = DispatcherQueue.CreateTimer();
        timer.Interval = TimeSpan.FromSeconds(1);
        timer.Tick += (_, _) => FetchDetails();
        timer.Start();
        Closed += (_, _) =>
        {
            closed = true;
            timer.Stop();
            store.RowsChanged -= OnRowsChanged;
            store.RowsRemoved -= OnRowsRemoved;
            Open.Remove(id);
            OpenChanged?.Invoke();
        };
        Update();
        FetchDetails();
    }

    private void OnRowsChanged(IReadOnlyList<RowChange> changes)
    {
        foreach (var change in changes)
        {
            if (change.Item.Id == id) row = change.Item.Row;
        }
        Update();
    }

    private void OnRowsRemoved(IReadOnlySet<string> ids)
    {
        if (ids.Contains(id)) Close();
    }

    private void Update()
    {
        if (row is null)
        {
            Title = "Starting\u2026";
            NameText.Text = "Starting\u2026";
            return;
        }
        var complete = row.Status == TaskStatus.Complete;
        Title = complete ? "Download Complete" : string.Join(" ", new[] { Format.Percent(row), row.Name }.Where(part => part.Length > 0));
        FileGlyph.Glyph = Format.Glyph(row.Category);
        NameText.Text = row.Name;
        AddressText.Text = row.Url;
        ToolTipService.SetToolTip(AddressText, row.Url);
        var switched = (CompletePanel.Visibility == Visibility.Visible) != complete;
        RunningPanel.Visibility = complete ? Visibility.Collapsed : Visibility.Visible;
        CompletePanel.Visibility = complete ? Visibility.Visible : Visibility.Collapsed;
        CloseButton.Visibility = complete ? Visibility.Visible : Visibility.Collapsed;
        if (complete)
        {
            SizeText.Text = row.TotalBytes > 0 ? Format.Bytes(row.TotalBytes) : "\u2014";
            SavedToText.Text = row.Path ?? row.Dir;
            ToolTipService.SetToolTip(SavedToText, SavedToText.Text);
            PrimaryButton.Content = "Open";
            SecondaryButton.Content = "Show in Folder";
            ToolTipService.SetToolTip(SecondaryButton, null);
            Grid.SetColumn(PrimaryButton, 0);
            Grid.SetColumn(SecondaryButton, 1);
        }
        else
        {
            var downloading = row.Status == TaskStatus.Downloading;
            var failed = Format.Shown(row) == TaskStatus.Failed;
            Bar.IsIndeterminate = row.Progress < 0 && downloading;
            Bar.Value = Math.Clamp(row.Progress, 0, 1);
            Bar.ShowPaused = Format.Shown(row) is TaskStatus.Paused or TaskStatus.Queued;
            Bar.ShowError = failed;
            StatusText.Text = Format.Status(row);
            StatusText.Foreground = failed ? Brush("SystemFillColorCriticalBrush") : Brush("TextFillColorPrimaryBrush");
            DownloadedText.Text = Dash(string.Join(" \u00B7 ", new[] { Format.Amount(row), Format.Percent(row) }.Where(part => part.Length > 0)));
            // These stay while paused so the window keeps its height.
            SpeedText.Text = Dash(downloading ? Format.Speed(row.Speed) : "");
            TimeLeftText.Text = Dash(downloading ? Format.TimeLeft(row.EtaSecs) : "");
            ConnectionsText.Text = Dash(downloading && row.Connections > 0 ? row.Connections.ToString() : "");
            PrimaryButton.Content = row.Status switch
            {
                TaskStatus.Paused => "Resume",
                TaskStatus.Failed => "Retry",
                _ => "Pause",
            };
            SecondaryButton.Content = "Cancel";
            ToolTipService.SetToolTip(SecondaryButton, "Stops the download; it stays in the list to resume later.");
            Grid.SetColumn(PrimaryButton, 1);
            Grid.SetColumn(SecondaryButton, 2);
            if (details is not null) ShowDetails(details);
        }
        if (switched) Refit();
    }

    private static string Dash(string text) => text.Length > 0 ? text : "\u2014";

    private static Brush Brush(string key) => (Brush)Application.Current.Resources[key];

    /// <summary>The segment map and connections, asked off the UI thread while the download runs.</summary>
    private void FetchDetails()
    {
        if (fetching || closed || row is null || (row.Status != TaskStatus.Downloading && details is not null)) return;
        fetching = true;
        Task.Run(() => core.TaskDetails(id, 120)).ContinueWith(task => DispatcherQueue.TryEnqueue(() =>
        {
            fetching = false;
            if (closed || !task.IsCompletedSuccessfully) return;
            details = task.Result;
            ShowDetails(details);
        }));
    }

    private void ShowDetails(TaskDetails details)
    {
        var segments = details.Segments;
        var hadMap = SegmentMap.Visibility == Visibility.Visible;
        SegmentMap.Visibility = segments.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
        if (segments.Length > 0)
        {
            if (cells.Count != segments.Length)
            {
                SegmentMap.Children.Clear();
                SegmentMap.ColumnDefinitions.Clear();
                cells.Clear();
                for (var i = 0; i < segments.Length; i++)
                {
                    SegmentMap.ColumnDefinitions.Add(new ColumnDefinition());
                    var cell = new Rectangle();
                    Grid.SetColumn(cell, i);
                    SegmentMap.Children.Add(cell);
                    cells.Add(cell);
                }
            }
            // The progress bar's color, so a paused download's parts turn with it.
            var fill = (row is null ? (TaskStatus?)null : Format.Shown(row)) switch
            {
                TaskStatus.Paused or TaskStatus.Queued => Brush("SystemFillColorCautionBrush"),
                TaskStatus.Failed => Brush("SystemFillColorCriticalBrush"),
                _ => Brush("AccentFillColorDefaultBrush"),
            };
            for (var i = 0; i < segments.Length; i++)
            {
                cells[i].Fill = fill;
                cells[i].Opacity = segments[i] == 0 ? 0 : 0.25 + 0.75 * segments[i] / 100.0;
            }
        }
        // The engine reports the servers in use, not each connection.
        var hosts = details.Connections.Select(connection => connection.Host).Distinct().Order().ToList();
        ServersText.Text = hosts.Count == 0 ? "" : (hosts.Count == 1 ? "Server: " : "Servers: ") + string.Join(", ", hosts);
        ServersText.Visibility = hosts.Count == 0 ? Visibility.Collapsed : Visibility.Visible;
        if (hadMap != (segments.Length > 0) && DetailsExpander.IsExpanded) Refit();
    }

    private void Refit() => DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, () => ToolWindows.Fit(this, ClientWidth));

    private void Details_Expanding(Expander sender, ExpanderExpandingEventArgs args) => RememberDetails(true);

    private void Details_Collapsed(Expander sender, ExpanderCollapsedEventArgs args) => RememberDetails(false);

    private void RememberDetails(bool shown)
    {
        using (var key = Registry.CurrentUser.CreateSubKey(DetailsKey))
        {
            key.SetValue("ProgressDetails", shown ? 1 : 0, RegistryValueKind.DWord);
        }
        Refit();
    }

    private void Primary_Click(object sender, RoutedEventArgs e)
    {
        if (row is null) return;
        switch (row.Status)
        {
            case TaskStatus.Complete:
                if (row.Path is { } path) Shell.Open(path);
                Close();
                break;
            case TaskStatus.Paused or TaskStatus.Failed:
                core.Resume([id]);
                break;
            default:
                core.Pause([id]);
                break;
        }
    }

    private void Secondary_Click(object sender, RoutedEventArgs e)
    {
        if (row?.Status == TaskStatus.Complete)
        {
            Shell.Reveal(row.Path, row.Dir);
        }
        else
        {
            // As in IDM: the download stops and stays in the list.
            core.Pause([id]);
        }
        Close();
    }

    private void Close_Click(object sender, RoutedEventArgs e) => Close();
}
