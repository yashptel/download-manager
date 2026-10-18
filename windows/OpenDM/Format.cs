using System.Globalization;

namespace OpenDM;

internal static class Format
{
    private static readonly string[] Units = ["B", "KB", "MB", "GB", "TB"];

    public static string Bytes(ulong count)
    {
        var (value, unit) = Scaled(count);
        return unit == 0
            ? $"{count} B"
            : value.ToString(value < 10 ? "0.0" : "0", CultureInfo.CurrentCulture) + " " + Units[unit];
    }

    /// <summary>
    /// A byte count rounded the way <see cref="Bytes"/> shows it, so sorting by
    /// speed does not reorder rows whose speeds look the same.
    /// </summary>
    public static double Displayed(ulong count)
    {
        var (value, unit) = Scaled(count);
        return unit == 0 ? count : Math.Round(value, value < 10 ? 1 : 0) * Math.Pow(1024, unit);
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

    public static string Percent(TaskRow row) => row.Progress < 0 ? "" : $"{(int)Math.Floor(row.Progress * 100)}%";

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

    /// <summary>"88.1 MB of 209.7 MB", or what is known so far.</summary>
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
        _ => row.Error ?? "Failed",
    };

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
