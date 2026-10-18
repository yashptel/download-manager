using System.ComponentModel;

namespace OpenDM;

/// <summary>
/// One row of the download list. The store updates it in place and each
/// displayed value that changed raises its own notification, so a progress
/// tick touches only the cells whose text or bar actually changed.
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
    /// <summary>0 to 1; the width of the progress indicator.</summary>
    public double Fraction => Math.Clamp(Row.Progress, 0, 1);
    public bool IsIndeterminate => Row.Progress < 0 && Row.Status == TaskStatus.Downloading;
    /// <summary>Only progress that means something: none for finished rows or ones that have not started.</summary>
    public bool IsDeterminate => !IsIndeterminate && Row.Status != TaskStatus.Complete && (Row.Progress > 0 || Row.Status == TaskStatus.Downloading);
    public string PercentText => IsDeterminate ? Format.Percent(Row) : "";
    public string SpeedText => Format.Speed(Row.Speed);
    public string EtaText => Format.Eta(Row.EtaSecs);
    public string StatusText => Format.Status(Row);
    public bool IsPaused => Row.Status == TaskStatus.Paused;
    public bool IsFailed => Row.Status == TaskStatus.Failed;
    public bool IsNotFailed => !IsFailed;
    public bool IsRunningOrDone => !IsPaused && !IsFailed;

    internal void Update(TaskRow row)
    {
        var (name, glyph, size, fraction, indeterminate, determinate, percent) = (Name, Glyph, SizeText, Fraction, IsIndeterminate, IsDeterminate, PercentText);
        var (speed, eta, status, paused, failed) = (SpeedText, EtaText, StatusText, IsPaused, IsFailed);
        Row = row;
        if (Name != name) Raise(nameof(Name));
        if (Glyph != glyph) Raise(nameof(Glyph));
        if (SizeText != size) Raise(nameof(SizeText));
        if (Fraction != fraction) Raise(nameof(Fraction));
        if (IsIndeterminate != indeterminate) Raise(nameof(IsIndeterminate));
        if (IsDeterminate != determinate) Raise(nameof(IsDeterminate));
        if (PercentText != percent) Raise(nameof(PercentText));
        if (SpeedText != speed) Raise(nameof(SpeedText));
        if (EtaText != eta) Raise(nameof(EtaText));
        if (StatusText != status) Raise(nameof(StatusText));
        if (IsPaused != paused || IsFailed != failed)
        {
            Raise(nameof(IsPaused));
            Raise(nameof(IsFailed));
            Raise(nameof(IsNotFailed));
            Raise(nameof(IsRunningOrDone));
        }
    }

    private void Raise(string property) => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(property));
}
