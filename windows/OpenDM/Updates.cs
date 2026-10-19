using System.Net;
using System.Reflection;
using System.Text.Json;
using Microsoft.Win32;

namespace OpenDM;

/// <summary>A published version of OpenDM and the page to download it from.</summary>
internal sealed record Release(Version Version, string Page);

/// <summary>
/// Asks GitHub for the latest release. OpenDM checks when it starts and once a
/// day while it runs, unless the user turned that off in Settings; Settings
/// can also check at any time. OPENDM_UPDATE_URL points the check elsewhere
/// for testing.
/// </summary>
internal static class Updates
{
    private const string LatestRelease = "https://api.github.com/repos/yashptel/download-manager/releases/latest";
    private const string PreferencesKey = @"Software\OpenDM";

    /// <summary>This copy's version, from the build (Cargo.toml).</summary>
    public static Version Current { get; } =
        Parse(typeof(Updates).Assembly.GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion) ?? new Version(0, 0);

    public static bool Automatic
    {
        get => Registry.CurrentUser.OpenSubKey(PreferencesKey)?.GetValue("CheckForUpdates") is not 0;
        set
        {
            using var key = Registry.CurrentUser.CreateSubKey(PreferencesKey);
            key.SetValue("CheckForUpdates", value ? 1 : 0, RegistryValueKind.DWord);
        }
    }

    /// <summary>The latest release when it is newer than this copy, otherwise null.</summary>
    /// <exception cref="HttpRequestException">GitHub could not be asked; the message says why.</exception>
    public static async Task<Release?> CheckAsync()
    {
        using var http = new HttpClient { Timeout = TimeSpan.FromSeconds(20) };
        http.DefaultRequestHeaders.UserAgent.ParseAdd($"OpenDM/{Current}");
        http.DefaultRequestHeaders.Accept.ParseAdd("application/vnd.github+json");
        using var response = await http.GetAsync(Environment.GetEnvironmentVariable("OPENDM_UPDATE_URL") ?? LatestRelease);
        if (response.StatusCode == HttpStatusCode.NotFound)
        {
            throw new HttpRequestException("No release has been published yet.", null, HttpStatusCode.NotFound);
        }
        response.EnsureSuccessStatusCode();
        using var json = JsonDocument.Parse(await response.Content.ReadAsStringAsync());
        var latest = Parse(json.RootElement.GetProperty("tag_name").GetString());
        var page = json.RootElement.GetProperty("html_url").GetString();
        return latest is not null && page is not null && latest > Current ? new Release(latest, page) : null;
    }

    /// <summary>"v1.2.3", "1.2.3+commit" or "1.2.3-beta" as 1.2.3.</summary>
    private static Version? Parse(string? text)
    {
        var plain = (text ?? "").TrimStart('v', 'V').Split('+', '-')[0];
        return Version.TryParse(plain, out var version) ? version : null;
    }
}
