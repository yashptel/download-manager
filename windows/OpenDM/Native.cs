using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.Marshalling;

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

internal static partial class Native
{
    private static readonly Guid DownloadsFolderId = new("374DE290-123F-4565-9164-39C4925E467B");
    private static readonly ITaskbarList3? Taskbar = CreateTaskbar();

    /// <summary>TBPFLAG: how the taskbar button shows progress.</summary>
    internal enum TaskbarProgress : uint
    {
        None = 0,
        Indeterminate = 1,
        Normal = 2,
        Paused = 8,
    }

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

    /// <summary>Moves the window above all other windows without the focus. Windows allows this to any app.</summary>
    public static void PutOnTop(IntPtr window)
    {
        const uint flags = 0x1 | 0x2 | 0x10; // SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE
        SetWindowPos(window, new IntPtr(-1), 0, 0, 0, 0, flags); // HWND_TOPMOST
        SetWindowPos(window, new IntPtr(-2), 0, 0, 0, 0, flags); // HWND_NOTOPMOST: stays in front, no longer pinned
    }

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags);

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

    /// <summary>Shows progress on the window's taskbar button; a null fraction leaves the value as it was.</summary>
    public static void SetTaskbarProgress(IntPtr window, TaskbarProgress state, double? fraction = null)
    {
        if (window == IntPtr.Zero || Taskbar is null) return;
        if (Taskbar.SetProgressState(window, state) < 0 || fraction is not { } value) return;
        Taskbar.SetProgressValue(window, (ulong)Math.Round(Math.Clamp(value, 0, 1) * 1000), 1000);
    }

    // Native AOT has no built-in COM interop, so the interface goes through
    // the source-generated ComWrappers.
    private static ITaskbarList3? CreateTaskbar()
    {
        var classId = new Guid("56FDF344-FD6D-11D0-958A-006097C9A090"); // CLSID_TaskbarList
        var interfaceId = typeof(ITaskbarList3).GUID;
        if (CoCreateInstance(classId, IntPtr.Zero, 1, interfaceId, out var pointer) < 0) return null;
        try
        {
            var taskbar = (ITaskbarList3)new StrategyBasedComWrappers().GetOrCreateObjectForComInstance(pointer, CreateObjectFlags.None);
            return taskbar.HrInit() >= 0 ? taskbar : null;
        }
        finally
        {
            Marshal.Release(pointer);
        }
    }

    // ITaskbarList, ITaskbarList2 and ITaskbarList3 in vtable order, up to the methods used.
    [GeneratedComInterface]
    [Guid("EA1AFB91-9E28-4B86-90E9-9E9F8A5EEFAF")]
    internal partial interface ITaskbarList3
    {
        [PreserveSig] int HrInit();
        [PreserveSig] int AddTab(IntPtr window);
        [PreserveSig] int DeleteTab(IntPtr window);
        [PreserveSig] int ActivateTab(IntPtr window);
        [PreserveSig] int SetActiveAlt(IntPtr window);
        [PreserveSig] int MarkFullscreenWindow(IntPtr window, int fullscreen);
        [PreserveSig] int SetProgressValue(IntPtr window, ulong completed, ulong total);
        [PreserveSig] int SetProgressState(IntPtr window, TaskbarProgress state);
    }

    [LibraryImport("ole32.dll")]
    private static partial int CoCreateInstance(in Guid classId, IntPtr outer, uint context, in Guid interfaceId, out IntPtr instance);

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
