using System.Runtime.InteropServices;

namespace OpenDM;

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
}
