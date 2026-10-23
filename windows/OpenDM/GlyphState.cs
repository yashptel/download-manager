namespace OpenDM;

/// <summary>
/// What the tray and taskbar say about the downloads. A fraction is the
/// share of the known bytes that are done, or null when no download in that
/// state knows its size yet.
/// </summary>
internal abstract record GlyphState
{
    public const int Frames = 26;

    public sealed record Idle : GlyphState;
    public sealed record Downloading(double? Fraction) : GlyphState;
    public sealed record Paused(double? Fraction) : GlyphState;
    public sealed record Completed : GlyphState;

    public static GlyphState From(IEnumerable<TaskRow> rows)
    {
        var all = rows.ToArray();
        var active = all.Where(row => row.Status == TaskStatus.Downloading).ToArray();
        if (active.Length > 0) return new Downloading(DoneShare(active));
        var paused = all.Where(row => row.Status == TaskStatus.Paused).ToArray();
        return paused.Length > 0 ? new Paused(DoneShare(paused)) : new Idle();
    }

    private static double? DoneShare(TaskRow[] rows)
    {
        var known = rows.Where(row => row.TotalBytes > 0).ToArray();
        var total = known.Sum(row => (double)row.TotalBytes);
        if (total <= 0) return null;
        return Math.Clamp(known.Sum(row => (double)row.DoneBytes) / total, 0, 1);
    }

    /// <summary>How many of the arrow's chunks are filled in, or null for the plain icon.</summary>
    public int? Frame => this switch
    {
        Downloading { Fraction: { } fraction } => Index(fraction),
        Paused { Fraction: { } fraction } => Index(fraction),
        Completed => Frames - 1,
        _ => null,
    };

    public bool IsDownloading => this is Downloading;

    private static int Index(double fraction) => (int)Math.Floor(Math.Clamp(fraction, 0, 1) * (Frames - 1));
}
