using System.Diagnostics;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;

namespace OpenDM;

/// <param name="Old">The previous row, or null for a row new to the list.</param>
internal readonly record struct RowChange(TaskRow? Old, DownloadItem Item);

/// <summary>
/// UI-thread copy of the task list. The core sends only the rows that
/// changed; the store updates their items and tells the window what changed.
/// </summary>
internal sealed class DownloadStore
{
    private readonly Dictionary<string, DownloadItem> byId = new();
    private readonly List<DownloadItem> items = new();

    public DownloadStore(DispatcherQueue dispatcher)
    {
        Listener = new CoreEvents(this, dispatcher);
        if (TimingPath is not null)
        {
            TimeSpan? last = null;
            CompositionTarget.Rendering += (_, args) =>
            {
                var now = ((RenderingEventArgs)args).RenderingTime;
                if (last is { } previous && (now - previous).TotalMilliseconds > 20)
                {
                    File.AppendAllText(TimingPath, $"-1,{(now - previous).TotalMilliseconds:F1}\n");
                }
                last = now;
            };
        }
    }

    public CoreListener Listener { get; }

    /// <summary>Oldest first.</summary>
    public IReadOnlyList<DownloadItem> Items => items;

    public GlobalStats Stats { get; private set; } = new(0, 0, 0, 0);

    /// <summary>The current row of a download, or null when it is not in the list.</summary>
    public TaskRow? Row(string id) => byId.TryGetValue(id, out var item) ? item.Row : null;

    public EngineState EngineState { get; private set; } = new EngineState.Stopped();

    public event Action<IReadOnlyList<RowChange>>? RowsChanged;
    public event Action<IReadOnlySet<string>>? RowsRemoved;
    public event Action? StatusChanged;
    public event Action<string>? ErrorRaised;
    /// <summary>Downloads the browser extension handed over.</summary>
    public event Action<NewDownload[], BrowserSource>? BrowserDownloadsReceived;
    /// <summary>A server wants a user name and password for download (id, url).</summary>
    public event Action<string, string>? LoginNeeded;
    /// <summary>The new address for a download waiting for one (id, download).</summary>
    public event Action<string, NewDownload>? NewAddressReceived;

    /// <summary>
    /// With OPENDM_UI_TIMING set to a file path, each batch of changed rows
    /// appends "rows,total,apply" there: the UI-thread milliseconds to apply
    /// it, the total including the layout pass it causes in
    /// <see cref="LayoutRoot"/>. Frames longer than 20 ms append "-1,ms".
    /// </summary>
    private static readonly string? TimingPath = Environment.GetEnvironmentVariable("OPENDM_UI_TIMING");

    public UIElement? LayoutRoot { get; set; }

    public void Load(DownloadCore core)
    {
        items.Clear();
        byId.Clear();
        foreach (var row in core.Rows())
        {
            Add(row);
        }
        Stats = core.Stats();
        EngineState = core.EngineState();
    }

    private DownloadItem Add(TaskRow row)
    {
        var item = new DownloadItem(row);
        byId[row.Id] = item;
        items.Add(item);
        return item;
    }

    private void Apply(TaskRow[] rows)
    {
        var changes = new List<RowChange>(rows.Length);
        foreach (var row in rows)
        {
            if (byId.TryGetValue(row.Id, out var item))
            {
                var old = item.Row;
                item.Update(row);
                changes.Add(new RowChange(old, item));
            }
            else
            {
                changes.Add(new RowChange(null, Add(row)));
            }
        }
        RowsChanged?.Invoke(changes);
    }

    private void Remove(string[] ids)
    {
        var removed = ids.ToHashSet();
        items.RemoveAll(item => removed.Contains(item.Id));
        foreach (var id in ids)
        {
            byId.Remove(id);
        }
        RowsRemoved?.Invoke(removed);
    }

    /// <summary>Receives core events on a background thread and replays them, in order, on the UI thread.</summary>
    private sealed class CoreEvents(DownloadStore store, DispatcherQueue dispatcher) : CoreListener
    {
        public void RowsChanged(TaskRow[] rows) => Post(() =>
        {
            if (TimingPath is null)
            {
                store.Apply(rows);
                return;
            }
            var start = Stopwatch.GetTimestamp();
            store.Apply(rows);
            var applied = Stopwatch.GetElapsedTime(start).TotalMilliseconds;
            store.LayoutRoot?.UpdateLayout();
            var elapsed = Stopwatch.GetElapsedTime(start).TotalMilliseconds;
            File.AppendAllText(TimingPath, $"{rows.Length},{elapsed:F3},{applied:F3}\n");
        });

        public void RowsRemoved(string[] ids) => Post(() => store.Remove(ids));

        public void StatsChanged(GlobalStats stats) => Post(() =>
        {
            store.Stats = stats;
            store.StatusChanged?.Invoke();
        });

        public void EngineStateChanged(EngineState state) => Post(() =>
        {
            store.EngineState = state;
            store.StatusChanged?.Invoke();
        });

        public void Error(string message) => Post(() => store.ErrorRaised?.Invoke(message));

        public void BrowserDownloads(NewDownload[] downloads, BrowserSource source) =>
            Post(() => store.BrowserDownloadsReceived?.Invoke(downloads, source));

        public void LoginNeeded(string id, string url) => Post(() => store.LoginNeeded?.Invoke(id, url));

        public void NewAddress(string id, NewDownload download) =>
            Post(() => store.NewAddressReceived?.Invoke(id, download));

        private void Post(Action action) => dispatcher.TryEnqueue(() => action());
    }
}
