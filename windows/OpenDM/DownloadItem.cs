using System.ComponentModel;

namespace OpenDM;

/// <summary>
/// One row of the download list. The store updates it in place and each
/// displayed value that changed raises its own notification, so a progress
/// tick touches only the cells whose text or bar actually changed.
/// </summary>
public sealed partial class DownloadItem : INotifyPropertyChanged
{
    /// <summary>Every value a row style shows, compared before and after an update.</summary>
    private static readonly (string Name, Func<DownloadItem, object?> Value)[] Shown =
    [
        (nameof(Name), item => item.Name),
        (nameof(Glyph), item => item.Glyph),
        (nameof(SizeText), item => item.SizeText),
        (nameof(Fraction), item => item.Fraction),
        (nameof(IsIndeterminate), item => item.IsIndeterminate),
        (nameof(IsDeterminate), item => item.IsDeterminate),
        (nameof(PercentText), item => item.PercentText),
        (nameof(SpeedText), item => item.SpeedText),
        (nameof(EtaText), item => item.EtaText),
        (nameof(Status), item => item.Status),
        (nameof(StatusTitle), item => item.StatusTitle),
        (nameof(Error), item => item.Error),
        (nameof(IsPaused), item => item.IsPaused),
        (nameof(IsFailed), item => item.IsFailed),
        (nameof(IsRunningOrDone), item => item.IsRunningOrDone),
        (nameof(DetailText), item => item.DetailText),
        (nameof(CardStatusText), item => item.CardStatusText),
        (nameof(CardAmountText), item => item.CardAmountText),
        (nameof(CardFraction), item => item.CardFraction),
        (nameof(PrimaryGlyph), item => item.PrimaryGlyph),
        (nameof(PrimaryLabel), item => item.PrimaryLabel),
        (nameof(IsUnfinished), item => item.IsUnfinished),
    ];

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
    /// <summary>For the row's colors and symbols.</summary>
    internal TaskStatus Status => Format.Shown(Row);
    public string StatusTitle => Format.Retrying(Row) ?? Format.Title(Row.Status);
    public string? Error => Row.Status == TaskStatus.Failed ? Row.Error : null;
    public bool IsPaused => Row.Status == TaskStatus.Paused;
    public bool IsFailed => Status == TaskStatus.Failed;
    public bool IsRunningOrDone => !IsPaused && !IsFailed;
    public string DetailText => Format.Detail(Row);
    public string CardStatusText => Format.CardStatus(Row);
    public string CardAmountText => Format.CardAmount(Row);
    /// <summary>A card's bar is always there; full once complete.</summary>
    public double CardFraction => Row.Status == TaskStatus.Complete ? 1 : Fraction;
    public bool IsUnfinished => Row.Status != TaskStatus.Complete;

    /// <summary>The row's main button: pause, resume, retry or open.</summary>
    public string PrimaryGlyph => Status switch
    {
        TaskStatus.Downloading or TaskStatus.Queued => "\uE769",
        TaskStatus.Paused => "\uE768",
        TaskStatus.Failed => "\uE72C",
        _ => "\uE8A7",
    };

    public string PrimaryLabel => Status switch
    {
        TaskStatus.Downloading or TaskStatus.Queued => "Pause",
        TaskStatus.Paused => "Resume",
        TaskStatus.Failed => "Retry",
        _ => "Open",
    };

    internal void Update(TaskRow row)
    {
        var before = new object?[Shown.Length];
        for (var i = 0; i < Shown.Length; i++)
        {
            before[i] = Shown[i].Value(this);
        }
        Row = row;
        for (var i = 0; i < Shown.Length; i++)
        {
            if (!Equals(before[i], Shown[i].Value(this))) Raise(Shown[i].Name);
        }
    }

    /// <summary>Status colors come from the theme; after it changes, rows look them up again.</summary>
    internal void RefreshLooks() => Raise(nameof(Status));

    private void Raise(string property) => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(property));
}

