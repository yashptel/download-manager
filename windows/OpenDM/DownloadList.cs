using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.ComponentModel;

namespace OpenDM;

/// <summary>
/// The filtered, sorted rows the ListView shows. A small change becomes the
/// fewest removes and inserts, so row containers, scroll position and the
/// selection of unmoved rows survive; larger changes are a single Reset
/// instead of thousands of notifications. Moves are sent as a remove and an
/// insert because the WinUI ListView treats a Move like a Reset: it re-binds
/// every realized row and drops the whole selection.
/// </summary>
public sealed class DownloadList : ObservableCollection<DownloadItem>
{
    private const int MaxChanges = 64;

    /// <returns>
    /// The rows whose selection the ListView forgot: every row after a Reset,
    /// otherwise the rows that moved.
    /// </returns>
    public IReadOnlyList<DownloadItem> ApplyOrder(IReadOnlyList<DownloadItem> next)
    {
        if (TryUpdate(next) is { } moved)
        {
            return moved;
        }
        Items.Clear();
        foreach (var item in next)
        {
            Items.Add(item);
        }
        OnPropertyChanged(new PropertyChangedEventArgs(nameof(Count)));
        OnPropertyChanged(new PropertyChangedEventArgs("Item[]"));
        OnCollectionChanged(new NotifyCollectionChangedEventArgs(NotifyCollectionChangedAction.Reset));
        return next;
    }

    private List<DownloadItem>? TryUpdate(IReadOnlyList<DownloadItem> next)
    {
        var target = new Dictionary<DownloadItem, int>(next.Count, ReferenceEqualityComparer.Instance);
        for (var i = 0; i < next.Count; i++)
        {
            target[next[i]] = i;
        }
        var removed = this.Count(item => !target.ContainsKey(item));
        if (removed > MaxChanges || Math.Abs(next.Count - Count) > MaxChanges) return null;

        // Rows that keep their place: the longest run already in the target order.
        var kept = LongestIncreasingRun(this.Where(target.ContainsKey).Select(item => target[item]).ToList());
        var placed = next.Count - kept.Count;
        if (removed + placed > MaxChanges) return null;

        for (var i = Count - 1; i >= 0; i--)
        {
            if (!target.ContainsKey(this[i])) RemoveAt(i);
        }
        // Each other row goes right after its predecessor in the target order.
        var moved = new List<DownloadItem>();
        for (var i = 0; i < next.Count; i++)
        {
            if (kept.Contains(i)) continue;
            var from = IndexOf(next[i]);
            if (from >= 0)
            {
                if (from == (i == 0 ? 0 : IndexOf(next[i - 1]) + 1)) continue;
                RemoveAt(from);
                moved.Add(next[i]);
            }
            Insert(i == 0 ? 0 : IndexOf(next[i - 1]) + 1, next[i]);
        }
        return moved;
    }

    /// <summary>The values of the longest strictly increasing subsequence.</summary>
    private static HashSet<int> LongestIncreasingRun(List<int> values)
    {
        // tails[k]: index into values of the smallest tail of an increasing run of length k + 1.
        var tails = new List<int>();
        var previous = new int[values.Count];
        for (var i = 0; i < values.Count; i++)
        {
            int low = 0, high = tails.Count;
            while (low < high)
            {
                var middle = (low + high) / 2;
                if (values[tails[middle]] < values[i]) low = middle + 1; else high = middle;
            }
            previous[i] = low > 0 ? tails[low - 1] : -1;
            if (low == tails.Count) tails.Add(i); else tails[low] = i;
        }
        var run = new HashSet<int>();
        for (var i = tails.Count > 0 ? tails[^1] : -1; i >= 0; i = previous[i])
        {
            run.Add(values[i]);
        }
        return run;
    }
}
