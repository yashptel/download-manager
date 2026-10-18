using System.Runtime.InteropServices;
using System.Text;

namespace OpenDM;

/// <summary>
/// A saved user name for a site that asks for one (HTTP 401). The site is a
/// host, optionally with a path ("example.com/private") to cover only that
/// part of it. The password stays in Windows Credential Manager.
/// </summary>
public sealed record SiteLogin(string Site, string User);

/// <summary>
/// Site logins in Windows Credential Manager, as generic credentials named
/// "OpenDM site login:&lt;site&gt;:&lt;user&gt;" with the user name in the credential.
/// </summary>
internal static class SiteLogins
{
    private const string Prefix = "OpenDM site login:";
    private const uint GenericType = 1;
    private const uint PersistLocalMachine = 2;

    public static List<SiteLogin> All()
    {
        var logins = new List<SiteLogin>();
        if (!CredEnumerateW(Prefix + "*", 0, out var count, out var list)) return logins;
        try
        {
            for (var i = 0; i < count; i++)
            {
                var credential = Marshal.PtrToStructure<Credential>(Marshal.ReadIntPtr(list, i * IntPtr.Size));
                var user = credential.UserName ?? "";
                var target = credential.TargetName;
                if (!target.StartsWith(Prefix) || !target.EndsWith(":" + user)) continue;
                logins.Add(new SiteLogin(target[Prefix.Length..^(user.Length + 1)], user));
            }
        }
        finally
        {
            CredFree(list);
        }
        return logins.OrderBy(login => login.Site).ThenBy(login => login.User).ToList();
    }

    public static string? Password(SiteLogin login)
    {
        if (!CredReadW(Target(login), GenericType, 0, out var pointer)) return null;
        try
        {
            var credential = Marshal.PtrToStructure<Credential>(pointer);
            return Marshal.PtrToStringUni(credential.CredentialBlob, (int)credential.CredentialBlobSize / 2);
        }
        finally
        {
            CredFree(pointer);
        }
    }

    /// <summary>Saves or replaces the password of a site and user name.</summary>
    public static void Save(SiteLogin login, string password)
    {
        var blob = Encoding.Unicode.GetBytes(password);
        var handle = GCHandle.Alloc(blob, GCHandleType.Pinned);
        try
        {
            var credential = new Credential
            {
                Type = GenericType,
                TargetName = Target(login),
                Comment = "OpenDM: " + login.Site,
                CredentialBlobSize = (uint)blob.Length,
                CredentialBlob = handle.AddrOfPinnedObject(),
                Persist = PersistLocalMachine,
                UserName = login.User,
            };
            if (!CredWriteW(ref credential, 0)) throw new System.ComponentModel.Win32Exception();
        }
        finally
        {
            handle.Free();
        }
    }

    public static void Remove(SiteLogin login) => CredDeleteW(Target(login), GenericType, 0);

    /// <summary>The saved login that covers <paramref name="url"/> most closely: the same host, and the longest path that starts the URL's path.</summary>
    public static SiteLogin? Match(string url)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri)) return null;
        var host = uri.Host.ToLowerInvariant();
        return All()
            .Where(login =>
            {
                var (siteHost, sitePath) = Split(login.Site);
                return siteHost == host && (sitePath.Length == 0 || uri.AbsolutePath.StartsWith(sitePath));
            })
            .MaxBy(login => login.Site.Length);
    }

    /// <summary>"example.com/private/" from what was typed, for storing and matching.</summary>
    public static string Normalize(string site)
    {
        var text = site.Trim();
        var scheme = text.IndexOf("://", StringComparison.Ordinal);
        if (scheme >= 0) text = text[(scheme + 3)..];
        var (host, path) = Split(text);
        return host.ToLowerInvariant() + path;
    }

    /// <summary>The host of a URL, which a remembered login covers.</summary>
    public static string SiteOf(string url) =>
        Uri.TryCreate(url, UriKind.Absolute, out var uri) ? uri.Host.ToLowerInvariant() : url;

    private static (string Host, string Path) Split(string site)
    {
        var slash = site.IndexOf('/');
        return slash < 0 ? (site, "") : (site[..slash], site[slash..]);
    }

    private static string Target(SiteLogin login) => $"{Prefix}{login.Site}:{login.User}";

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct Credential
    {
        public uint Flags;
        public uint Type;
        public string TargetName;
        public string? Comment;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
        public uint CredentialBlobSize;
        public IntPtr CredentialBlob;
        public uint Persist;
        public uint AttributeCount;
        public IntPtr Attributes;
        public string? TargetAlias;
        public string? UserName;
    }

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CredWriteW(ref Credential credential, uint flags);

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CredReadW(string target, uint type, uint flags, out IntPtr credential);

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CredEnumerateW(string filter, uint flags, out int count, out IntPtr credentials);

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CredDeleteW(string target, uint type, uint flags);

    [DllImport("advapi32.dll")]
    private static extern void CredFree(IntPtr buffer);
}

/// <summary>
/// What happens when a server asks for a login: a saved one is tried first,
/// once per download; otherwise, or when it fails, the user is asked.
/// </summary>
internal static class LoginPrompt
{
    /// <summary>Downloads that have had a saved or typed login since the last ask.</summary>
    private static readonly HashSet<string> Tried = new();
    private static readonly HashSet<string> Asking = new();

    public static void Handle(string id, string url, DownloadStore store, DownloadCore core)
    {
        var saved = SiteLogins.Match(url);
        if (saved is not null && !Tried.Contains(id) && SiteLogins.Password(saved) is { } password)
        {
            Tried.Add(id);
            core.SetLogin(id, saved.User, password);
            return;
        }
        if (!Asking.Add(id)) return;
        var name = store.Row(id)?.Name ?? url;
        var window = new LoginWindow(url, name, saved?.User ?? "", answer =>
        {
            Asking.Remove(id);
            if (answer is null) return;
            Tried.Add(id);
            if (answer.Remember)
            {
                try
                {
                    SiteLogins.Save(new SiteLogin(SiteLogins.SiteOf(url), answer.User), answer.Password);
                }
                catch (System.ComponentModel.Win32Exception error)
                {
                    Native.ShowError("The login could not be saved", error.Message);
                }
            }
            core.SetLogin(id, answer.User, answer.Password);
        });
        ToolWindows.Show(window, activate: true);
    }
}

internal sealed record LoginAnswer(string User, string Password, bool Remember);
