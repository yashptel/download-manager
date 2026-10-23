namespace OpenDM;

internal abstract record GlyphState
{
    public sealed record Idle : GlyphState;
    public sealed record Downloading(double Fraction) : GlyphState;
    public sealed record Paused : GlyphState;
    public sealed record Completed : GlyphState;

    public static GlyphState From(IEnumerable<TaskRow> rows)
    {
        var active = rows.Where(row => row.Status == TaskStatus.Downloading).ToArray();
        if (active.Length > 0)
        {
            var known = active.Where(row => row.TotalBytes > 0).ToArray();
            var total = known.Sum(row => (double)row.TotalBytes);
            var done = known.Sum(row => (double)row.DoneBytes);
            var fraction = total > 0 ? Math.Clamp(done / total, 0, 1) : 0;
            return new Downloading(fraction);
        }
        return rows.Any(row => row.Status == TaskStatus.Paused) ? new Paused() : new Idle();
    }

    public static int FrameIndex(GlyphState state) => state switch
    {
        Downloading downloading => (int)Math.Floor(Math.Clamp(downloading.Fraction, 0, 1) * 25),
        Completed => 25,
        _ => 0,
    };

    public bool IsDownloading => this is Downloading;
}
