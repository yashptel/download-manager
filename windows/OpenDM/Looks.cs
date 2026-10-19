using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;

namespace OpenDM;

/// <summary>
/// Status symbols and colors for the list's rows: the accent color while
/// downloading, gray while queued, orange when paused, green when complete,
/// red when failed. Bound with x:Bind functions; the list asks rows to look
/// again when the theme changes.
/// </summary>
internal static class Looks
{
    /// <summary>Segoe Fluent Icons glyph for a status.</summary>
    public static string Glyph(TaskStatus status) => status switch
    {
        TaskStatus.Downloading => "\uE896",
        TaskStatus.Queued => "\uE823",
        TaskStatus.Paused => "\uE769",
        TaskStatus.Complete => "\uE73E",
        _ => "\uE7BA",
    };

    /// <summary>For the status symbol and text.</summary>
    public static Brush Text(TaskStatus status) => Resource(status switch
    {
        TaskStatus.Downloading => "AccentTextFillColorPrimaryBrush",
        TaskStatus.Queued => "TextFillColorSecondaryBrush",
        TaskStatus.Paused => "SystemFillColorCautionBrush",
        TaskStatus.Complete => "SystemFillColorSuccessBrush",
        _ => "SystemFillColorCriticalBrush",
    });

    /// <summary>For the progress bar.</summary>
    public static Brush Bar(TaskStatus status) => status switch
    {
        TaskStatus.Downloading => Resource("AccentFillColorDefaultBrush"),
        TaskStatus.Queued => Resource("ControlStrongFillColorDefaultBrush"),
        _ => Text(status),
    };

    /// <summary>For a detail line: red with an error, secondary otherwise.</summary>
    public static Brush Detail(TaskStatus status) =>
        Resource(status == TaskStatus.Failed ? "SystemFillColorCriticalBrush" : "TextFillColorSecondaryBrush");

    private static Brush Resource(string key) => (Brush)Application.Current.Resources[key];
}
