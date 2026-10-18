using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.ComponentModel;

namespace OpenDM;

/// <summary>
/// The filtered, sorted rows the ListView shows. A small reorder becomes a
/// few Move notifications, so row containers and selection survive; larger
/// changes are a single Reset instead of thousands of notifications.
/// </summary>
public sealed class DownloadList : ObservableCollection<DownloadItem>
{
    private const int MaxMoves = 64;

    /// <returns>True when the list was reset, which clears the selection.</returns>
    public bool ApplyOrder(IReadOnlyList<DownloadItem> next)
    {
        if (next.Count == Count && TryMove(next, out var reset))
        {
            return reset;
        }
        Items.Clear();
        foreach (var item in next)
        {
            Items.Add(item);
        }
        OnPropertyChanged(new PropertyChangedEventArgs(nameof(Count)));
        OnPropertyChanged(new PropertyChangedEventArgs("Item[]"));
        OnCollectionChanged(new NotifyCollectionChangedEventArgs(NotifyCollectionChangedAction.Reset));
        return true;
    }

    private bool TryMove(IReadOnlyList<DownloadItem> next, out bool reset)
    {
        reset = false;
        var current = new HashSet<DownloadItem>(ReferenceEqualityComparer.Instance);
        var wanted = new HashSet<DownloadItem>(ReferenceEqualityComparer.Instance);
        for (var i = 0; i < next.Count; i++)
        {
            if (ReferenceEquals(this[i], next[i])) continue;
            current.Add(this[i]);
            wanted.Add(next[i]);
            if (current.Count > MaxMoves) return false;
        }
        if (current.Count == 0) return true;
        if (!current.SetEquals(wanted)) return false;
        for (var i = 0; i < next.Count; i++)
        {
            if (ReferenceEquals(this[i], next[i])) continue;
            var from = i + 1;
            while (!ReferenceEquals(this[from], next[i])) from++;
            Move(from, i);
        }
        return true;
    }
}
