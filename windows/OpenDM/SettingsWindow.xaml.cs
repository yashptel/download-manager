using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Globalization;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.Win32;
using Windows.Graphics;
using WinRT.Interop;

namespace OpenDM;

/// <summary>
/// The Settings window: General for everyday choices, the other pages for
/// the engine's details. Changes are saved half a second after the last one.
/// </summary>
public sealed partial class SettingsWindow : Window
{
    private static SettingsWindow? shared;

    private readonly DownloadCore core;
    private readonly DispatcherQueueTimer saveTimer;
    private readonly ObservableCollection<CategoryItem> categories = new();
    private readonly ObservableCollection<SiteLogin> logins = new();
    private Settings settings;
    private bool loading = true;

    internal static void Show(DownloadCore core)
    {
        shared ??= new SettingsWindow(core);
        shared.Activate();
        Native.SetForegroundWindow(WindowNative.GetWindowHandle(shared));
    }

    private SettingsWindow(DownloadCore core)
    {
        this.core = core;
        InitializeComponent();
        settings = core.Settings();
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "OpenDM.ico"));
        AppWindow.TitleBar.PreferredTheme = TitleBarTheme.UseDefaultAppMode;
        var scale = Native.GetDpiForWindow(WindowNative.GetWindowHandle(this)) / 96.0;
        var size = new SizeInt32((int)(940 * scale), (int)(720 * scale));
        var area = DisplayArea.GetFromWindowId(AppWindow.Id, DisplayAreaFallback.Primary).WorkArea;
        AppWindow.MoveAndResize(new RectInt32(area.X + (area.Width - size.Width) / 2, area.Y + (area.Height - size.Height) / 2, size.Width, size.Height));

        saveTimer = DispatcherQueue.CreateTimer();
        saveTimer.Interval = TimeSpan.FromSeconds(0.5);
        saveTimer.IsRepeating = false;
        saveTimer.Tick += (_, _) => Save();
        Closed += (_, _) =>
        {
            if (saveTimer.IsRunning)
            {
                saveTimer.Stop();
                Save();
            }
            shared = null;
        };

        Load();
        Nav.SelectedItem = Nav.MenuItems[0];
        loading = false;
    }

    /// <summary>Puts the settings in the controls and connects each control to its setting.</summary>
    private void Load()
    {
        DownloadDirText.Text = settings.DownloadDir;
        ToolTipService.SetToolTip(DownloadDirText, settings.DownloadDir);
        Switch(CategoryFoldersSwitch, settings.CategoryFolders, (s, on) => s with { CategoryFolders = on });
        Number(ConcurrentBox, settings.MaxConcurrentDownloads, (s, n) => s with { MaxConcurrentDownloads = (uint)n });
        Switch(ConfirmSwitch, settings.ConfirmDownloads, (s, on) => s with { ConfirmDownloads = on });
        Switch(ProgressSwitch, settings.ShowProgressWindow, (s, on) => s with { ShowProgressWindow = on });
        StartupSwitch.IsOn = StartAtSignIn.IsOn;
        VersionText.Text = $"OpenDM {Updates.Current}";
        AutoUpdateSwitch.IsOn = Updates.Automatic;
        AutoUpdateSwitch.Toggled += (_, _) => Updates.Automatic = AutoUpdateSwitch.IsOn;

        Switch(CaptureSwitch, settings.BrowserCapture, (s, on) => s with { BrowserCapture = on });
        Text(ExtensionsBox, string.Join(" ", settings.CaptureExtensions), (s, text) => s with { CaptureExtensions = Words(text) });
        Text(ExcludedBox, string.Join(Environment.NewLine, settings.CaptureExcludedSites), (s, text) => s with { CaptureExcludedSites = Lines(text) });
        Number(MinSizeBox, settings.CaptureMinSize / 1_000_000, (s, n) => s with { CaptureMinSize = (ulong)n * 1_000_000 });

        foreach (var rule in settings.Categories)
        {
            categories.Add(new CategoryItem(rule, SaveCategories));
        }
        CategoryList.ItemsSource = categories;

        Number(MaxSpeedBox, settings.MaxDownloadSpeed / 1024, (s, n) => s with { MaxDownloadSpeed = (ulong)n * 1024 });
        Number(TaskSpeedBox, settings.MaxTaskSpeed / 1024, (s, n) => s with { MaxTaskSpeed = (ulong)n * 1024 });
        Number(ConnectionsBox, settings.MaxConnectionsPerServer, (s, n) => s with { MaxConnectionsPerServer = (uint)n });
        Number(SplitBox, settings.Split, (s, n) => s with { Split = (uint)n });
        Text(MinSplitBox, settings.MinSplitSize, (s, text) => s with { MinSplitSize = text.Trim() });
        Number(TriesBox, settings.MaxTries, (s, n) => s with { MaxTries = (uint)n });
        Number(RetryWaitBox, settings.RetryWait, (s, n) => s with { RetryWait = (uint)n });
        Number(AutoRetriesBox, settings.AutoRetries, (s, n) => s with { AutoRetries = (uint)n });
        Number(TimeoutBox, settings.Timeout, (s, n) => s with { Timeout = (uint)n });
        Number(ConnectTimeoutBox, settings.ConnectTimeout, (s, n) => s with { ConnectTimeout = (uint)n });
        Text(ProxyBox, settings.Proxy, (s, text) => s with { Proxy = text.Trim() });
        Text(NoProxyBox, settings.NoProxy, (s, text) => s with { NoProxy = text.Trim() });
        Text(UserAgentBox, settings.UserAgent, (s, text) => s with { UserAgent = text.Trim() });
        Text(HeadersBox, string.Join(Environment.NewLine, settings.CustomHeaders), (s, text) => s with { CustomHeaders = Lines(text) });
        FileExistsBox.SelectedIndex = (int)settings.FileExists;
        FileExistsBox.SelectionChanged += (_, _) => Change(s => s with { FileExists = (FileExists)FileExistsBox.SelectedIndex });

        LoginList.ItemsSource = logins;
        ReloadLogins();

        var schedule = settings.Schedule;
        Switch(ScheduleSwitch, schedule.Enabled, (s, on) => s with { Schedule = s.Schedule with { Enabled = on } });
        ScheduleSwitch.Toggled += (_, _) => ShowSchedule();
        StartPicker.Time = Time(schedule.Start);
        StartPicker.SelectedTimeChanged += (_, e) => Change(s => s with { Schedule = s.Schedule with { Start = Time(e.NewTime) } });
        PauseSwitch.IsOn = schedule.Stop.Length > 0;
        PauseSwitch.Toggled += (_, _) =>
        {
            var stop = PauseSwitch.IsOn ? Time(StopPicker.Time) : "";
            Change(s => s with { Schedule = s.Schedule with { Stop = stop } });
            ShowSchedule();
        };
        StopPicker.Time = schedule.Stop.Length > 0 ? Time(schedule.Stop) : new TimeSpan(8, 0, 0);
        StopPicker.SelectedTimeChanged += (_, e) => Change(s => s with { Schedule = s.Schedule with { Stop = Time(e.NewTime) } });
        AddDays(schedule.Days);
        ShowSchedule();

        Text(EngineBox, settings.EngineExtra, (s, text) => s with { EngineExtra = text });
    }

    private void Change(Func<Settings, Settings> change)
    {
        if (loading) return;
        settings = change(settings);
        saveTimer.Stop();
        saveTimer.Start();
    }

    private async void Save()
    {
        try
        {
            core.SetSettings(settings);
        }
        catch (CoreException.Failed error)
        {
            if (Content.XamlRoot is null) return;
            await new ContentDialog
            {
                Title = "The settings could not be saved",
                Content = error.message,
                CloseButtonText = "OK",
                XamlRoot = Content.XamlRoot,
            }.ShowAsync();
        }
    }

    private void Switch(ToggleSwitch toggle, bool value, Func<Settings, bool, Settings> set)
    {
        toggle.IsOn = value;
        toggle.Toggled += (_, _) => Change(s => set(s, toggle.IsOn));
    }

    /// <summary>A whole number; an emptied box goes back to its last value.</summary>
    private void Number(NumberBox box, double value, Func<Settings, double, Settings> set)
    {
        box.Value = value;
        box.ValueChanged += (_, e) =>
        {
            if (double.IsNaN(e.NewValue))
            {
                box.Value = e.OldValue;
                return;
            }
            Change(s => set(s, Math.Round(e.NewValue)));
        };
    }

    private void Text(TextBox box, string value, Func<Settings, string, Settings> set)
    {
        box.Text = value;
        box.TextChanged += (_, _) => Change(s => set(s, box.Text));
    }

    /// <summary>Extensions separated by spaces or commas, lowercase and without dots.</summary>
    private static string[] Words(string text) =>
        text.Split([' ', ',', '\r', '\n', '\t'], StringSplitOptions.RemoveEmptyEntries)
            .Select(word => word.Trim('.').ToLowerInvariant())
            .Where(word => word.Length > 0)
            .ToArray();

    private static string[] Lines(string text) =>
        text.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);

    private void Nav_SelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (args.SelectedItem is not NavigationViewItem item) return;
        var page = (string)item.Tag;
        PageTitle.Text = (string)item.Content;
        foreach (var (name, panel) in new[]
        {
            ("General", GeneralPage), ("Browser", BrowserPage), ("Categories", CategoriesPage), ("Connection", ConnectionPage),
            ("Logins", LoginsPage), ("Scheduler", SchedulerPage), ("Engine", EnginePage),
        })
        {
            panel.Visibility = name == page ? Visibility.Visible : Visibility.Collapsed;
        }
        Scroller.ChangeView(null, 0, null, disableAnimation: true);
    }

    // General

    private async void DownloadDir_Click(object sender, RoutedEventArgs e)
    {
        if (await NewDownloads.ChooseFolder(this) is not { } folder) return;
        DownloadDirText.Text = folder;
        ToolTipService.SetToolTip(DownloadDirText, folder);
        Change(s => s with { DownloadDir = folder });
    }

    private void StartupSwitch_Toggled(object sender, RoutedEventArgs e)
    {
        if (loading || StartupSwitch.IsOn == StartAtSignIn.IsOn) return;
        StartAtSignIn.Set(StartupSwitch.IsOn);
    }

    private async void CheckUpdates_Click(object sender, RoutedEventArgs e)
    {
        CheckButton.IsEnabled = false;
        UpdateLink.Visibility = Visibility.Collapsed;
        UpdateText.Visibility = Visibility.Visible;
        UpdateText.Text = "Checking\u2026";
        try
        {
            if (await Updates.CheckAsync() is { } release)
            {
                UpdateText.Text = $"OpenDM {release.Version} is available.";
                UpdateLink.NavigateUri = new Uri(release.Page);
                UpdateLink.Visibility = Visibility.Visible;
            }
            else
            {
                UpdateText.Text = "You have the latest version.";
            }
        }
        catch (HttpRequestException error) when (error.StatusCode == System.Net.HttpStatusCode.NotFound)
        {
            // Nothing failed: there is just nothing newer to offer.
            UpdateText.Text = error.Message;
        }
        catch (HttpRequestException error)
        {
            UpdateText.Text = $"Could not check for updates. {error.Message}";
        }
        catch (TaskCanceledException)
        {
            UpdateText.Text = "Could not check for updates. GitHub did not answer in time.";
        }
        finally
        {
            CheckButton.IsEnabled = true;
        }
    }

    // Categories

    private void SaveCategories()
    {
        var rules = categories.Select(item => item.Rule).ToArray();
        Change(s => s with { Categories = rules });
    }

    private void CategoryList_SelectionChanged(object sender, SelectionChangedEventArgs e) =>
        RemoveCategoryButton.IsEnabled = CategoryList.SelectedItem is not null;

    /// <summary>Typing in a row selects it, so Remove acts on the row being edited.</summary>
    private void CategoryField_GotFocus(object sender, RoutedEventArgs e) =>
        CategoryList.SelectedItem = ((FrameworkElement)sender).DataContext;

    private async void CategoryFolder_Click(object sender, RoutedEventArgs e)
    {
        if (((FrameworkElement)sender).DataContext is not CategoryItem item) return;
        CategoryList.SelectedItem = item;
        if (await NewDownloads.ChooseFolder(this) is { } folder) item.Folder = folder;
    }

    private void AddCategory_Click(object sender, RoutedEventArgs e)
    {
        var item = new CategoryItem(new CategoryRule("New Category", [], "New Category"), SaveCategories);
        categories.Add(item);
        CategoryList.SelectedItem = item;
        CategoryList.ScrollIntoView(item);
        SaveCategories();
    }

    private void RemoveCategory_Click(object sender, RoutedEventArgs e)
    {
        if (CategoryList.SelectedItem is not CategoryItem item) return;
        categories.Remove(item);
        SaveCategories();
    }

    // Logins

    private void ReloadLogins()
    {
        logins.Clear();
        foreach (var login in SiteLogins.All())
        {
            logins.Add(login);
        }
        NoLoginsText.Visibility = logins.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    private void LoginList_SelectionChanged(object sender, SelectionChangedEventArgs e) =>
        RemoveLoginButton.IsEnabled = LoginList.SelectedItem is not null;

    private void RemoveLogin_Click(object sender, RoutedEventArgs e)
    {
        if (LoginList.SelectedItem is not SiteLogin login) return;
        SiteLogins.Remove(login);
        ReloadLogins();
    }

    private async void AddLogin_Click(object sender, RoutedEventArgs e)
    {
        var site = new TextBox { Header = "Site", PlaceholderText = "example.com or example.com/private" };
        var user = new TextBox { Header = "User name" };
        var password = new PasswordBox { Header = "Password" };
        var panel = new StackPanel { Spacing = 12, Width = 360 };
        panel.Children.Add(site);
        panel.Children.Add(user);
        panel.Children.Add(password);
        var dialog = new ContentDialog
        {
            Title = "Add a Login",
            Content = panel,
            PrimaryButtonText = "Add",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
            IsPrimaryButtonEnabled = false,
            XamlRoot = Content.XamlRoot,
        };
        void Validate() => dialog.IsPrimaryButtonEnabled = SiteLogins.Normalize(site.Text).Length > 0 && user.Text.Trim().Length > 0;
        site.TextChanged += (_, _) => Validate();
        user.TextChanged += (_, _) => Validate();
        if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;
        try
        {
            SiteLogins.Save(new SiteLogin(SiteLogins.Normalize(site.Text), user.Text.Trim()), password.Password);
        }
        catch (Win32Exception error)
        {
            Native.ShowError("The login could not be saved", error.Message);
        }
        ReloadLogins();
    }

    // Scheduler

    private void ShowSchedule()
    {
        ScheduleDetails.Visibility = ScheduleSwitch.IsOn ? Visibility.Visible : Visibility.Collapsed;
        PauseCard.Visibility = PauseSwitch.IsOn ? Visibility.Visible : Visibility.Collapsed;
    }

    /// <summary>One button per day, in the order of the user's week; days are stored with 0 as Sunday.</summary>
    private void AddDays(byte[] chosen)
    {
        var names = CultureInfo.CurrentCulture.DateTimeFormat;
        var first = (int)names.FirstDayOfWeek;
        for (var i = 0; i < 7; i++)
        {
            var day = (byte)((first + i) % 7);
            var button = new ToggleButton
            {
                Content = names.ShortestDayNames[day],
                IsChecked = chosen.Contains(day),
                MinWidth = 40,
            };
            ToolTipService.SetToolTip(button, names.DayNames[day]);
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(button, names.DayNames[day]);
            button.Click += (_, _) =>
            {
                var on = button.IsChecked == true;
                Change(s =>
                {
                    var days = s.Schedule.Days.Where(other => other != day);
                    if (on) days = days.Append(day);
                    return s with { Schedule = s.Schedule with { Days = days.Order().ToArray() } };
                });
            };
            DaysPanel.Children.Add(button);
        }
    }

    private static TimeSpan Time(string text) =>
        TimeSpan.TryParseExact(text, @"hh\:mm", CultureInfo.InvariantCulture, out var time) ? time : TimeSpan.Zero;

    private static string Time(TimeSpan? time) => (time ?? TimeSpan.Zero).ToString(@"hh\:mm", CultureInfo.InvariantCulture);
}

/// <summary>One row of the categories table, edited in place.</summary>
public sealed partial class CategoryItem : INotifyPropertyChanged
{
    private readonly Action changed;
    private string name;
    private string extensions;
    private string folder;

    internal CategoryItem(CategoryRule rule, Action changed)
    {
        this.changed = changed;
        name = rule.Name;
        extensions = string.Join(" ", rule.Extensions);
        folder = rule.Folder;
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    public string Name
    {
        get => name;
        set => Set(ref name, value, nameof(Name));
    }

    public string Extensions
    {
        get => extensions;
        set => Set(ref extensions, value, nameof(Extensions));
    }

    public string Folder
    {
        get => folder;
        set => Set(ref folder, value, nameof(Folder));
    }

    internal CategoryRule Rule => new(
        name.Trim(),
        extensions.Split([' ', ','], StringSplitOptions.RemoveEmptyEntries).Select(word => word.Trim('.').ToLowerInvariant()).ToArray(),
        folder);

    private void Set(ref string field, string value, string property)
    {
        if (field == value) return;
        field = value;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(property));
        changed();
    }
}

/// <summary>
/// Whether Windows opens OpenDM when the user signs in: a value under the
/// Run key, unless Task Manager's Startup apps turned it off.
/// </summary>
internal static class StartAtSignIn
{
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string ApprovedKey = @"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    private const string Name = "OpenDM";

    public static bool IsOn
    {
        get
        {
            using var run = Registry.CurrentUser.OpenSubKey(RunKey);
            if (run?.GetValue(Name) is not string) return false;
            // Task Manager marks a disabled startup app with an odd first byte.
            using var approved = Registry.CurrentUser.OpenSubKey(ApprovedKey);
            return approved?.GetValue(Name) is not byte[] { Length: > 0 } state || (state[0] & 1) == 0;
        }
    }

    public static void Set(bool on)
    {
        using var run = Registry.CurrentUser.CreateSubKey(RunKey);
        using var approved = Registry.CurrentUser.OpenSubKey(ApprovedKey, writable: true);
        approved?.DeleteValue(Name, throwOnMissingValue: false);
        if (on)
        {
            run.SetValue(Name, $"\"{Environment.ProcessPath}\" --background");
        }
        else
        {
            run.DeleteValue(Name, throwOnMissingValue: false);
        }
    }
}
