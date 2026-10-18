using System.ComponentModel;

namespace OpenDM;

/// <summary>
/// One row of the download list. The store updates it in place and each
/// changed field raises its own notification, so a progress tick redraws
/// only the cells that changed.
/// </summary>
public sealed class DownloadItem : INotifyPropertyChanged
{
    internal DownloadItem(TaskRow row)
    {
        Row = row;
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    internal TaskRow Row { get; private set; }

    public string Id => Row.Id;
    public string Name => Row.Name;
    public string Url => Row.Url;
    public string Glyph => Format.Glyph(Row.Category);
    public string SizeText => Format.Size(Row);
    public double ProgressValue => Row.Progress < 0 ? 0 : Row.Progress * 100;
    public bool IsIndeterminate => Row.Progress < 0 && Row.Status == TaskStatus.Downloading;
    public string PercentText => Format.Percent(Row);
    public string SpeedText => Format.Speed(Row.Speed);
    public string EtaText => Format.Eta(Row.EtaSecs);
    public string StatusText => Format.Status(Row);
    public bool IsPaused => Row.Status == TaskStatus.Paused;
    public bool IsFailed => Row.Status == TaskStatus.Failed;
    public bool IsNotFailed => !IsFailed;

    internal void Update(TaskRow row)
    {
        var old = Row;
        Row = row;
        if (old.Name != row.Name) Raise(nameof(Name));
        if (old.Category != row.Category) Raise(nameof(Glyph));
        if (old.TotalBytes != row.TotalBytes) Raise(nameof(SizeText));
        if (old.Progress != row.Progress || old.Status != row.Status)
        {
            Raise(nameof(ProgressValue));
            Raise(nameof(IsIndeterminate));
            Raise(nameof(PercentText));
        }
        if (old.Speed != row.Speed) Raise(nameof(SpeedText));
        if (old.EtaSecs != row.EtaSecs) Raise(nameof(EtaText));
        if (old.Status != row.Status || old.Error != row.Error)
        {
            Raise(nameof(StatusText));
            Raise(nameof(IsPaused));
            Raise(nameof(IsFailed));
            Raise(nameof(IsNotFailed));
        }
    }

    private void Raise(string property) => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(property));
}
