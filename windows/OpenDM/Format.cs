using System.Globalization;

namespace OpenDM;

internal static class Format
{
    private static readonly string[] Units = ["B", "KB", "MB", "GB", "TB"];

    public static string Bytes(ulong count)
    {
        var (value, unit) = Scaled(count);
        return unit == 0 ? $"{count} B" : value.ToString("0.00", CultureInfo.CurrentCulture) + " " + Units[unit];
    }

    /// <summary>
    /// A byte count rounded the way <see cref="Bytes"/> shows it, so sorting by
    /// speed does not reorder rows whose speeds look the same.
    /// </summary>
    public static double Displayed(ulong count)
    {
        var (value, unit) = Scaled(count);
        return unit == 0 ? count : Math.Round(value, 2) * Math.Pow(1024, unit);
    }

    private static (double Value, int Unit) Scaled(ulong count)
    {
        double value = count;
        var unit = 0;
        while (value >= 1024 && unit < Units.Length - 1)
        {
            value /= 1024;
            unit++;
        }
        return (value, unit);
    }

    public static string Size(TaskRow row) => row.TotalBytes > 0 ? Bytes(row.TotalBytes) : "–";

    public static string Speed(ulong bytesPerSecond) => bytesPerSecond > 0 ? Bytes(bytesPerSecond) + "/s" : "";

    /// <summary>"44.53%"; rounded down, so 100.00% means done.</summary>
    public static string Percent(TaskRow row) =>
        row.Progress < 0 ? "" : (Math.Floor(row.Progress * 10_000) / 100).ToString("0.00", CultureInfo.CurrentCulture) + "%";

    public static string Eta(ulong? seconds) => seconds switch
    {
        null => "",
        < 60 => $"{seconds}s",
        < 3600 => $"{seconds / 60}m {seconds % 60}s",
        < 86_400 => $"{seconds / 3600}h {seconds % 3600 / 60}m",
        _ => $"{seconds / 86_400}d {seconds % 86_400 / 3600}h",
    };

    /// <summary>"45 sec", "4 min 47 sec", "2 hr 5 min", "3 days 4 hr": the progress window's time left.</summary>
    public static string TimeLeft(ulong? seconds) => seconds switch
    {
        null => "",
        < 60 => $"{seconds} sec",
        < 3600 => $"{seconds / 60} min {seconds % 60} sec",
        < 86_400 => $"{seconds / 3600} hr {seconds % 3600 / 60} min",
        _ => $"{seconds / 86_400} {(seconds / 86_400 == 1 ? "day" : "days")} {seconds % 86_400 / 3600} hr",
    };

    /// <summary>"88.12 MB of 209.70 MB", or what is known so far.</summary>
    public static string Amount(TaskRow row) => (row.DoneBytes, row.TotalBytes) switch
    {
        (0, 0) => "",
        (var done, 0) => Bytes(done),
        (0, var total) => Bytes(total),
        (var done, var total) when done >= total => Bytes(total),
        (var done, var total) => $"{Bytes(done)} of {Bytes(total)}",
    };

    public static string Status(TaskRow row) => row.Status switch
    {
        TaskStatus.Downloading => "Downloading",
        TaskStatus.Queued => "Queued",
        TaskStatus.Paused => "Paused",
        TaskStatus.Complete => "Completed",
        _ => Join([row.Error ?? "Failed", Retrying(row)]),
    };

    /// <summary>The status a row looks like: a download waiting to try again looks queued, not failed.</summary>
    public static TaskStatus Shown(TaskRow row) => row.RetryAt is null ? row.Status : TaskStatus.Queued;

    /// <summary>"Retrying at 10:52", for a failed download the app tries again on its own.</summary>
    public static string? Retrying(TaskRow row) => row.RetryAt is { } at
        ? "Retrying at " + DateTimeOffset.FromUnixTimeMilliseconds(at).LocalDateTime.ToString("t", CultureInfo.CurrentCulture)
        : null;

    /// <summary>The status in one word, "Failed" included.</summary>
    public static string Title(TaskStatus status) => status switch
    {
        TaskStatus.Downloading => "Downloading",
        TaskStatus.Queued => "Queued",
        TaskStatus.Paused => "Paused",
        TaskStatus.Complete => "Completed",
        _ => "Failed",
    };

    /// <summary>"45 sec left", "3 min left", "2 hr 5 min left".</summary>
    public static string? Remaining(ulong? seconds) => seconds switch
    {
        null => null,
        < 60 => $"{seconds} sec left",
        < 3600 => $"{seconds / 60} min left",
        _ => $"{seconds / 3600} hr {seconds % 3600 / 60} min left",
    };

    /// <summary>One line describing where a download stands: "82.41 MB of 209.70 MB · 258.03 KB/s · 8 min left".</summary>
    public static string Detail(TaskRow row) => Join(row.Status switch
    {
        TaskStatus.Downloading => [Amount(row), Speed(row.Speed), Remaining(row.EtaSecs)],
        TaskStatus.Queued => ["Waiting", Amount(row)],
        TaskStatus.Paused => ["Paused", Amount(row)],
        TaskStatus.Complete => [row.TotalBytes > 0 ? Bytes(row.TotalBytes) : "Completed"],
        _ => [row.Error ?? "Failed", Retrying(row)],
    });

    /// <summary>A card's status line: the status, its speed and time left while downloading, or the error.</summary>
    public static string CardStatus(TaskRow row) => row.Status switch
    {
        TaskStatus.Downloading => Join([Title(row.Status), Speed(row.Speed), Remaining(row.EtaSecs)]),
        TaskStatus.Failed => Join([row.Error ?? "Failed", Retrying(row)]),
        _ => Title(row.Status),
    };

    /// <summary>A card's size line: "39.30% · 82.41 MB / 209.70 MB"; "Not started" while waiting with nothing yet.</summary>
    public static string CardAmount(TaskRow row)
    {
        if (row.Status == TaskStatus.Complete) return Size(row);
        if (row.DoneBytes == 0 && row.TotalBytes == 0)
        {
            return row.Status is TaskStatus.Queued or TaskStatus.Paused ? "Not started" : "";
        }
        var size = row.TotalBytes == 0
            ? Bytes(row.DoneBytes)
            : $"{Bytes(Math.Min(row.DoneBytes, row.TotalBytes))} / {Bytes(row.TotalBytes)}";
        return Join([Percent(row), size]);
    }

    private static string Join(string?[] parts) => string.Join(" \u00B7 ", parts.Where(part => !string.IsNullOrEmpty(part)));

    /// <summary>What waits when nothing downloads: "2 paused", "1 queued · 3 paused"; empty when nothing does.</summary>
    public static string Idle(IReadOnlyList<DownloadItem> items)
    {
        var queued = items.Count(item => item.Row.Status == TaskStatus.Queued);
        var paused = items.Count(item => item.Row.Status == TaskStatus.Paused);
        return string.Join(" \u00B7 ", new[] { queued > 0 ? $"{queued} queued" : null, paused > 0 ? $"{paused} paused" : null }.OfType<string>());
    }

    /// <summary>Segoe Fluent Icons glyph for a category.</summary>
    public static string Glyph(string category) => category switch
    {
        "Compressed" => "\uF012",
        "Documents" => "\uE8A5",
        "Music" => "\uE8D6",
        "Programs" => "\uE7B8",
        "Video" => "\uE714",
        _ => "\uE8B7",
    };
}

internal static class Links
{
    private static readonly string[] Prefixes = ["http://", "https://", "sftp://", "magnet:?"];

    /// <summary>Every whitespace-separated token that looks like a download link.</summary>
    public static string[] Find(string? text) =>
        (text ?? "")
            .Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries)
            .Where(token => Prefixes.Any(prefix => token.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)))
            .ToArray();
}
