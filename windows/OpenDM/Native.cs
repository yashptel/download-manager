using System.Diagnostics;
using System.Runtime.InteropServices;

namespace OpenDM;

/// <summary>Opening downloaded files and showing them in Explorer.</summary>
internal static class Shell
{
    public static void Open(string path)
    {
        try
        {
            Process.Start(new ProcessStartInfo(Path.GetFullPath(path)) { UseShellExecute = true });
        }
        catch (Exception error)
        {
            Debug.WriteLine($"OpenDM: could not open {path}: {error.Message}");
        }
    }

    /// <summary>Selects the file in Explorer, or opens its folder when the file is not there.</summary>
    public static void Reveal(string? path, string dir)
    {
        var file = path is null ? null : Path.GetFullPath(path);
        var arguments = file is not null && File.Exists(file)
            ? $"/select,\"{file}\""
            : $"\"{Path.GetFullPath(dir)}\"";
        Process.Start("explorer.exe", arguments);
    }
}

internal static class Native
{
    private static readonly Guid DownloadsFolderId = new("374DE290-123F-4565-9164-39C4925E467B");

    /// <summary>The user's Downloads folder, including a relocated one.</summary>
    public static string DownloadsFolder()
    {
        var id = DownloadsFolderId;
        if (SHGetKnownFolderPath(ref id, 0, IntPtr.Zero, out var path) == 0)
        {
            try
            {
                return Marshal.PtrToStringUni(path) ?? DefaultDownloads();
            }
            finally
            {
                Marshal.FreeCoTaskMem(path);
            }
        }
        return DefaultDownloads();
    }

    private static string DefaultDownloads() =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Downloads");

    /// <summary>Explorer's natural order: "file2" before "file10".</summary>
    public static int CompareNatural(string a, string b) => StrCmpLogicalW(a, b);

    public static void ShowError(string title, string message) => MessageBoxW(IntPtr.Zero, message, title, 0x10);

    [DllImport("shell32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern int SHGetKnownFolderPath(ref Guid id, uint flags, IntPtr token, out IntPtr path);

    [DllImport("shlwapi.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern int StrCmpLogicalW(string a, string b);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern int MessageBoxW(IntPtr window, string text, string caption, uint type);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool SetForegroundWindow(IntPtr window);

    [DllImport("user32.dll")]
    public static extern uint GetDpiForWindow(IntPtr window);

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    /// <summary>Flashes the window's taskbar button until it comes to the front.</summary>
    public static void Flash(IntPtr window)
    {
        var info = new FlashInfo
        {
            Size = (uint)Marshal.SizeOf<FlashInfo>(),
            Window = window,
            Flags = 0x2 | 0xC, // FLASHW_TRAY | FLASHW_TIMERNOFG
        };
        FlashWindowEx(ref info);
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct FlashInfo
    {
        public uint Size;
        public IntPtr Window;
        public uint Flags;
        public uint Count;
        public uint Timeout;
    }

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool FlashWindowEx(ref FlashInfo info);
}
