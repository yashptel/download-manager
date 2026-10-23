using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Input;
using Windows.Graphics;
using Windows.System;
using WinRT.Interop;

namespace OpenDM;

/// <summary>
/// Small fixed-size windows: the download dialogs, progress windows and
/// prompts. They cannot be resized or maximized, take their height from
/// their content, cascade from the last one open, and map Enter and Esc to
/// their default and cancel actions.
/// </summary>
internal static class ToolWindows
{
    private static readonly List<Window> Open = new();

    /// <param name="width">Client width in effective pixels.</param>
    /// <param name="topmost">Stays above other windows, like a confirmation panel.</param>
    public static void Setup(Window window, double width, bool topmost = false, bool minimizable = false)
    {
        window.AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "OpenDM.ico"));
        window.AppWindow.TitleBar.PreferredTheme = TitleBarTheme.UseDefaultAppMode;
        var presenter = OverlappedPresenter.CreateForDialog();
        presenter.IsAlwaysOnTop = topmost;
        presenter.IsMinimizable = minimizable;
        window.AppWindow.SetPresenter(presenter);
        // A first guess, so the window opens close to its final size.
        var scale = Scale(window);
        window.AppWindow.ResizeClient(new SizeInt32((int)(width * scale), (int)(300 * scale)));
        if (window.Content is FrameworkElement root)
        {
            root.Loaded += (_, _) => Fit(window, width);
        }
        window.Closed += (_, _) => Open.Remove(window);
    }

    /// <summary>Gives the window the height its content needs at this width.</summary>
    public static void Fit(Window window, double width)
    {
        if (window.Content is not FrameworkElement root || root.XamlRoot is null) return;
        root.Measure(new Windows.Foundation.Size(width, double.PositiveInfinity));
        var scale = root.XamlRoot.RasterizationScale;
        var size = new SizeInt32((int)Math.Ceiling(width * scale), (int)Math.Ceiling(root.DesiredSize.Height * scale));
        if (size != window.AppWindow.ClientSize) window.AppWindow.ResizeClient(size);
    }

    /// <summary>
    /// Shows the window centered, or cascaded from the last one open, in front.
    /// </summary>
    public static void Show(Window window)
    {
        Place(window);
        Open.Add(window);
        Raise(window);
    }

    /// <summary>
    /// Brings the window to the front with the focus. Windows gives the focus
    /// only to the app in use; otherwise the window goes above the other
    /// windows without it, and its taskbar button flashes.
    /// </summary>
    public static void Raise(Window window)
    {
        window.Activate();
        var hwnd = WindowNative.GetWindowHandle(window);
        if (Native.SetForegroundWindow(hwnd) || Native.GetForegroundWindow() == hwnd) return;
        Native.PutOnTop(hwnd);
        Native.Flash(hwnd);
    }

    /// <summary>Enter runs <paramref name="enter"/> unless a button or list has the focus; Esc runs <paramref name="escape"/>.</summary>
    public static void Keys(Window window, Action? enter, Action escape)
    {
        if (window.Content is not UIElement root) return;
        // KeyDown bubbles: buttons, combo boxes and multi-line text boxes
        // handle their own Enter before it gets here.
        root.KeyDown += (_, e) =>
        {
            if (e.Key == VirtualKey.Escape)
            {
                escape();
                e.Handled = true;
            }
            else if (e.Key == VirtualKey.Enter && enter is not null)
            {
                enter();
                e.Handled = true;
            }
        };
    }

    private static void Place(Window window)
    {
        var size = window.AppWindow.Size;
        var last = Open.LastOrDefault(other => other != window && other.AppWindow.IsVisible);
        if (last is not null)
        {
            var step = (int)(24 * Scale(window));
            window.AppWindow.Move(new PointInt32(last.AppWindow.Position.X + step, last.AppWindow.Position.Y + step));
            return;
        }
        var area = DisplayArea.GetFromWindowId(window.AppWindow.Id, DisplayAreaFallback.Primary).WorkArea;
        window.AppWindow.Move(new PointInt32(area.X + (area.Width - size.Width) / 2, area.Y + (area.Height - size.Height) / 3));
    }

    private static double Scale(Window window)
    {
        var dpi = Native.GetDpiForWindow(WindowNative.GetWindowHandle(window));
        return dpi > 0 ? dpi / 96.0 : 1.0;
    }
}
