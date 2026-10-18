import AppKit
import OpenDMCore
import ServiceManagement
import SwiftUI

/// The core's settings; SwiftUI has a Settings of its own.
typealias AppSettings = OpenDMCore.Settings

/// The Settings window: General for everyday choices, the other tabs for
/// the engine's details. Changes are saved as they are made.
@MainActor
final class SettingsWindowController: NSWindowController {
    private static var shared: SettingsWindowController?

    static func show(core: DownloadCore) {
        let controller = shared ?? SettingsWindowController(core: core)
        shared = controller
        controller.showWindow(nil)
        controller.window?.makeKeyAndOrderFront(nil)
        DispatchQueue.main.async { controller.window?.makeFirstResponder(nil) }
        NSApp.activate(ignoringOtherApps: true)
    }

    private init(core: DownloadCore) {
        let model = SettingsModel(core: core)
        let tabs = SettingsTabs()
        tabs.tabStyle = .toolbar
        func tab(_ title: String, _ symbol: String, _ view: some View) -> NSTabViewItem {
            let hosting = NSHostingController(rootView: view.environmentObject(model))
            hosting.sizingOptions = .preferredContentSize
            hosting.title = title
            let item = NSTabViewItem(viewController: hosting)
            item.label = title
            item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: title)
            return item
        }
        tabs.addTabViewItem(tab("General", "gearshape", GeneralSettings()))
        tabs.addTabViewItem(tab("Browser", "globe", BrowserSettings()))
        tabs.addTabViewItem(tab("Categories", "folder", CategorySettings()))
        tabs.addTabViewItem(tab("Connection", "network", ConnectionSettings()))
        tabs.addTabViewItem(tab("Logins", "key", LoginSettings()))
        tabs.addTabViewItem(tab("Scheduler", "calendar.badge.clock", SchedulerSettings()))
        tabs.addTabViewItem(tab("Engine", "terminal", EngineSettings()))
        let window = NSWindow(contentViewController: tabs)
        window.styleMask = [.titled, .closable]
        window.isReleasedWhenClosed = false
        window.center()
        super.init(window: window)
    }

    required init?(coder: NSCoder) { fatalError("not used") }
}

/// Tabs that open with nothing focused: a field selected on arrival reads
/// as if it were being edited.
private final class SettingsTabs: NSTabViewController {
    override func tabView(_ tabView: NSTabView, didSelect tabViewItem: NSTabViewItem?) {
        super.tabView(tabView, didSelect: tabViewItem)
        DispatchQueue.main.async { [weak self] in self?.view.window?.makeFirstResponder(nil) }
    }
}

/// The settings being edited, saved a moment after the last change.
@MainActor
final class SettingsModel: ObservableObject {
    private let core: DownloadCore
    private var saveTimer: Timer?

    @Published var settings: AppSettings {
        didSet {
            guard settings != oldValue else { return }
            saveTimer?.invalidate()
            saveTimer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: false) { [weak self] _ in
                MainActor.assumeIsolated { self?.save() }
            }
        }
    }

    init(core: DownloadCore) {
        self.core = core
        settings = core.settings()
        // Approval happens in System Settings, so look again on return.
        NotificationCenter.default.addObserver(forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.loginItem = LoginItem.current }
        }
    }

    // Login items live with macOS, not in the core's settings file.
    @Published var loginItem = LoginItem.current

    func setOpenAtLogin(_ on: Bool) {
        do {
            if on {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
        } catch {
            NSAlert(error: error).runModal()
        }
        loginItem = LoginItem.current
    }

    private func save() {
        do {
            try core.setSettings(settings: settings)
        } catch {
            let alert = NSAlert()
            alert.messageText = "The settings could not be saved"
            alert.informativeText = error.message
            alert.runModal()
        }
    }

    func chooseFolder(from path: String, then use: @escaping (String) -> Void) {
        let picker = NSOpenPanel()
        picker.canChooseDirectories = true
        picker.canChooseFiles = false
        picker.canCreateDirectories = true
        picker.prompt = "Choose"
        picker.directoryURL = URL(fileURLWithPath: path)
        if picker.runModal() == .OK, let url = picker.url { use(url.path) }
    }

    /// A whole-number setting as an Int, for steppers and number fields.
    func number<Value: BinaryInteger>(_ path: WritableKeyPath<AppSettings, Value>) -> Binding<Int> {
        Binding(get: { Int(self.settings[keyPath: path]) }, set: { self.settings[keyPath: path] = Value(clamping: max($0, 0)) })
    }

    /// Bytes per second as KB/s.
    func speed(_ path: WritableKeyPath<AppSettings, UInt64>) -> Binding<Int> {
        Binding(get: { Int(self.settings[keyPath: path] / 1024) }, set: { self.settings[keyPath: path] = UInt64(max($0, 0)) * 1024 })
    }

    /// A list of words as text, one per line or separated by spaces.
    func words(_ path: WritableKeyPath<AppSettings, [String]>, separator: String = " ") -> Binding<String> {
        Binding(
            get: { self.settings[keyPath: path].joined(separator: separator) },
            set: { text in
                let parts = separator == "\n"
                    ? text.split(whereSeparator: \.isNewline).map { $0.trimmingCharacters(in: .whitespaces) }
                    : text.split(whereSeparator: { $0.isWhitespace || $0 == "," }).map { $0.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: ".")) }
                self.settings[keyPath: path] = parts.filter { !$0.isEmpty }
            }
        )
    }
}

/// A folder path with a Choose button.
private struct FolderRow: View {
    let title: String
    @Binding var path: String
    @EnvironmentObject var model: SettingsModel

    var body: some View {
        LabeledContent(title) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(abbreviatedPath(path))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.head)
                    .help(path)
                Button("Choose…") { model.chooseFolder(from: path) { path = $0 } }
            }
        }
    }
}

/// A whole number in a bordered field with its unit after it. Every field
/// takes the same width and every unit the same slot, so the fields of a
/// group line up in one column.
private struct NumberField: View {
    @Binding var value: Int
    var unit = ""
    var range: ClosedRange<Int> = 0...Int(Int32.max)

    private var clamped: Binding<Int> {
        Binding(get: { value }, set: { value = min(max($0, range.lowerBound), range.upperBound) })
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            TextField("", value: clamped, format: .number)
                .labelsHidden()
                .textFieldStyle(.roundedBorder)
                .multilineTextAlignment(.trailing)
                .monospacedDigit()
                .frame(width: fieldWidth)
            UnitLabel(unit)
        }
    }
}

private let fieldWidth: CGFloat = 80

/// The slot after a field; empty units keep the field in the column.
private struct UnitLabel: View {
    let unit: String
    init(_ unit: String) { self.unit = unit }
    var body: some View {
        // A space, not "": empty text has no baseline, which makes the row taller.
        Text(unit.isEmpty ? " " : unit)
            .foregroundStyle(.secondary)
            .frame(width: 40, alignment: .leading)
            .accessibilityHidden(unit.isEmpty)
    }
}

/// A multi-line field that looks like one: inset text, the text
/// background and a hairline border.
private struct EditorBox: View {
    @Binding var text: String
    var height: CGFloat
    var monospaced = false

    var body: some View {
        TextEditor(text: $text)
            .font(monospaced ? .body.monospaced() : .body)
            .scrollContentBackground(.hidden)
            .padding(.horizontal, 4)
            .padding(.vertical, 6)
            .frame(height: height)
            .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 6))
            .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color(nsColor: .separatorColor)))
    }
}

/// Grey text under a group, as in System Settings.
/// Whether macOS opens OpenDM at login.
enum LoginItem {
    case off, on, needsApproval

    var isOn: Bool { self != .off }

    static var current: LoginItem {
        switch SMAppService.mainApp.status {
        case .enabled: .on
        case .requiresApproval: .needsApproval
        default: .off
        }
    }
}

private struct Note: View {
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text)
            .font(.callout)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// The width every tab shares, so switching tabs only changes the height.
private let tabWidth: CGFloat = 580

// MARK: - Tabs

private struct GeneralSettings: View {
    @EnvironmentObject var model: SettingsModel

    var body: some View {
        Form {
            Section {
                FolderRow(title: "Save downloads to", path: $model.settings.downloadDir)
                Toggle("Sort into category folders", isOn: $model.settings.categoryFolders)
                LabeledContent("Downloads at a time") {
                    NumberField(value: model.number(\.maxConcurrentDownloads), range: 1...20)
                }
            }
            Section {
                Toggle("Ask before starting it", isOn: $model.settings.confirmDownloads)
                Toggle("Show its progress window", isOn: $model.settings.showProgressWindow)
            } header: {
                Text("When a download is added")
            } footer: {
                Note("Asking shows the Download File Info dialog to name the file and pick its folder. Without it, downloads start at once.")
            }
            Section {
                Toggle("Open at login", isOn: Binding(get: { model.loginItem.isOn }, set: model.setOpenAtLogin))
            } footer: {
                if model.loginItem == .needsApproval {
                    VStack(alignment: .leading, spacing: 4) {
                        Note("macOS is waiting for you to allow OpenDM in Login Items.")
                        Button("Open Login Items") { SMAppService.openSystemSettingsLoginItems() }
                            .buttonStyle(.link)
                            .font(.callout)
                    }
                } else {
                    Note("OpenDM starts in the menu bar, ready for downloads from the browser. Closing its window keeps it running there; Quit stops it.")
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: tabWidth)
        .fixedSize(horizontal: false, vertical: true)
    }
}

private struct BrowserSettings: View {
    @EnvironmentObject var model: SettingsModel

    var body: some View {
        Form {
            Section {
                Toggle("Take over downloads from the browser", isOn: $model.settings.browserCapture)
            } footer: {
                Note("Needs the OpenDM Connect extension. Alt-click a link to keep that download in the browser.")
            }
            Section("File types to take over") {
                EditorBox(text: model.words(\.captureExtensions), height: 76)
            }
            Section {
                EditorBox(text: model.words(\.captureExcludedSites, separator: "\n"), height: 60)
            } header: {
                Text("Never take downloads from these sites")
            } footer: {
                Note("One per line. A site also covers its subdomains.")
            }
            Section {
                LabeledContent("Leave files smaller than") {
                    NumberField(
                        value: Binding(get: { Int(model.settings.captureMinSize / 1_000_000) },
                                       set: { model.settings.captureMinSize = UInt64(max($0, 0)) * 1_000_000 }),
                        unit: "MB"
                    )
                }
            } footer: {
                Note("0 takes files of any size.")
            }
        }
        .formStyle(.grouped)
        .frame(width: tabWidth)
        .fixedSize(horizontal: false, vertical: true)
    }
}

private struct CategorySettings: View {
    @EnvironmentObject var model: SettingsModel
    @State private var selection: Int?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: CategoryRow.spacing) {
                Text("Category").frame(width: CategoryRow.nameWidth, alignment: .leading)
                Text("File types").frame(maxWidth: .infinity, alignment: .leading)
                Text("Folder").frame(width: CategoryRow.folderWidth, alignment: .leading)
            }
            .font(.callout)
            .foregroundStyle(.secondary)
            .padding(.horizontal, CategoryRow.inset)
            List(selection: $selection) {
                ForEach(model.settings.categories.indices, id: \.self) { index in
                    CategoryRow(rule: $model.settings.categories[index], downloadDir: model.settings.downloadDir)
                        .tag(index)
                }
            }
            .listStyle(.bordered(alternatesRowBackgrounds: true))
            .frame(height: 230)
            HStack(spacing: 6) {
                Button { addCategory() } label: { Image(systemName: "plus").frame(width: 16, height: 16) }
                    .help("Add a category")
                Button { removeCategory() } label: { Image(systemName: "minus").frame(width: 16, height: 16) }
                    .help("Remove the selected category")
                    .disabled(selection == nil)
                Spacer()
            }
            Note("A file goes to the first category that lists its extension; the rest go to Other. A folder name without a path is inside the download folder.")
                .padding(.top, 4)
        }
        .padding(20)
        .frame(width: tabWidth + 80)
    }

    private func addCategory() {
        model.settings.categories.append(CategoryRule(name: "New Category", extensions: [], folder: "New Category"))
        selection = model.settings.categories.count - 1
    }

    private func removeCategory() {
        guard let selection, model.settings.categories.indices.contains(selection) else { return }
        model.settings.categories.remove(at: selection)
        self.selection = nil
    }
}

private struct CategoryRow: View {
    static let nameWidth: CGFloat = 130
    static let folderWidth: CGFloat = 210
    static let spacing: CGFloat = 10
    /// How far the bordered list's rows sit inside its border (measured on
    /// macOS 26), so the headings line up with the text below them.
    static let inset: CGFloat = 7

    @Binding var rule: CategoryRule
    let downloadDir: String
    @EnvironmentObject var model: SettingsModel

    private var extensions: Binding<String> {
        Binding(
            get: { rule.extensions.joined(separator: " ") },
            set: { text in
                rule.extensions = text.split(whereSeparator: { $0.isWhitespace || $0 == "," }).map { $0.lowercased() }
            }
        )
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Self.spacing) {
            TextField("Name", text: $rule.name)
                .labelsHidden()
                .frame(width: Self.nameWidth)
            TextField("Extensions", text: extensions)
                .labelsHidden()
                .frame(maxWidth: .infinity)
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(rule.folder)
                    .lineLimit(1)
                    .truncationMode(.head)
                    .help(rule.folder)
                    .frame(maxWidth: .infinity, alignment: .leading)
                Button("Choose…") {
                    let current = rule.folder.hasPrefix("/") ? rule.folder : (downloadDir as NSString).appendingPathComponent(rule.folder)
                    model.chooseFolder(from: current) { rule.folder = $0 }
                }
                .controlSize(.small)
            }
            .frame(width: Self.folderWidth)
        }
    }
}

/// IDM's Site Logins: user names and passwords for sites that ask (HTTP
/// 401), kept in the keychain.
private struct LoginSettings: View {
    @State private var logins = SiteLogins.all()
    @State private var selection: SiteLogin.ID?
    @State private var adding = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                Text("Site").frame(width: 300, alignment: .leading)
                Text("User name")
            }
            .font(.callout)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 7)
            List(logins, selection: $selection) { login in
                HStack(spacing: 10) {
                    Text(login.site).frame(width: 300, alignment: .leading).lineLimit(1).truncationMode(.middle)
                    Text(login.user).lineLimit(1)
                }
            }
            .listStyle(.bordered(alternatesRowBackgrounds: true))
            .frame(height: 200)
            .overlay {
                if logins.isEmpty {
                    Text("No saved logins").foregroundStyle(.secondary)
                }
            }
            HStack(spacing: 6) {
                Button { adding = true } label: { Image(systemName: "plus").frame(width: 16, height: 16) }
                    .help("Add a login")
                Button {
                    if let login = logins.first(where: { $0.id == selection }) { SiteLogins.remove(login) }
                    selection = nil
                    logins = SiteLogins.all()
                } label: { Image(systemName: "minus").frame(width: 16, height: 16) }
                    .help("Remove the selected login")
                    .disabled(selection == nil)
                Spacer()
            }
            Note("When a site asks for a user name and password, OpenDM uses the login saved for it, or asks and offers to remember it. A site can include a path, such as example.com/private, to cover only that part. Passwords are kept in your keychain.")
                .padding(.top, 4)
        }
        .padding(20)
        .frame(width: tabWidth + 80)
        .sheet(isPresented: $adding) {
            AddLoginSheet { login, password in
                do {
                    try SiteLogins.save(login, password: password)
                } catch {
                    NSAlert(error: error).runModal()
                }
                logins = SiteLogins.all()
            }
        }
    }
}

private struct AddLoginSheet: View {
    let save: (SiteLogin, String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var site = ""
    @State private var user = ""
    @State private var password = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Add a Login").font(.headline)
            Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 10, verticalSpacing: 10) {
                GridRow {
                    Text("Site:").gridColumnAlignment(.trailing)
                    TextField("example.com or example.com/private", text: $site).labelsHidden()
                }
                GridRow {
                    Text("User name:")
                    TextField("User name", text: $user).labelsHidden()
                }
                GridRow {
                    Text("Password:")
                    SecureField("Password", text: $password).labelsHidden()
                }
            }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Add") {
                    save(SiteLogin(site: SiteLogins.normalize(site), user: user.trimmingCharacters(in: .whitespaces)), password)
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(SiteLogins.normalize(site).isEmpty || user.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            .padding(.top, 8)
        }
        .padding(20)
        .frame(width: 420)
    }
}

private struct ConnectionSettings: View {
    @EnvironmentObject var model: SettingsModel

    var body: some View {
        Form {
            Section {
                LabeledContent("All downloads together") { NumberField(value: model.speed(\.maxDownloadSpeed), unit: "KB/s") }
                LabeledContent("Each download") { NumberField(value: model.speed(\.maxTaskSpeed), unit: "KB/s") }
            } header: {
                Text("Speed")
            } footer: {
                Note("0 means no limit.")
            }
            Section {
                LabeledContent("Connections per server") {
                    NumberField(value: model.number(\.maxConnectionsPerServer), range: 1...16)
                }
                LabeledContent("Split each file into") {
                    NumberField(value: model.number(\.split), unit: "parts", range: 1...64)
                }
                LabeledContent("Smallest part") {
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        TextField("", text: $model.settings.minSplitSize)
                            .labelsHidden()
                            .textFieldStyle(.roundedBorder)
                            .multilineTextAlignment(.trailing)
                            .frame(width: fieldWidth)
                        UnitLabel("")
                    }
                }
            } header: {
                Text("Connections")
            } footer: {
                Note("Smallest part takes sizes such as 1M or 512K. A file smaller than two parts downloads in one piece.")
            }
            Section {
                LabeledContent("Tries per server") { NumberField(value: model.number(\.maxTries), unit: "tries") }
                LabeledContent("Wait between tries") { NumberField(value: model.number(\.retryWait), unit: "sec") }
                LabeledContent("Give up on a silent connection after") { NumberField(value: model.number(\.timeout), unit: "sec") }
                LabeledContent("Give up connecting after") { NumberField(value: model.number(\.connectTimeout), unit: "sec") }
            } header: {
                Text("Retries")
            } footer: {
                Note("0 tries keeps trying until you stop the download.")
            }
            Section("Proxy") {
                TextField("Proxy", text: $model.settings.proxy, prompt: Text("http://user:password@host:8080"))
                TextField("Not for", text: $model.settings.noProxy, prompt: Text("localhost, .example.com"))
            }
            Section {
                TextField("User agent", text: $model.settings.userAgent, prompt: Text("The engine's own"))
                VStack(alignment: .leading, spacing: 6) {
                    Text("Headers for every download")
                    EditorBox(text: model.words(\.customHeaders, separator: "\n"), height: 58, monospaced: true)
                }
            } header: {
                Text("Requests")
            } footer: {
                Note("The user agent is for downloads you add yourself; the browser's keep the browser's. Headers are one \"Name: value\" per line.")
            }
            Section {
                Picker("When the file already exists", selection: $model.settings.fileExists) {
                    Text("Save under a new name").tag(FileExists.rename)
                    Text("Replace it").tag(FileExists.overwrite)
                    Text("Stop with an error").tag(FileExists.fail)
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: tabWidth, height: 640)
    }
}

private struct SchedulerSettings: View {
    @EnvironmentObject var model: SettingsModel
    private let days = Calendar.current.veryShortWeekdaySymbols
    private let dayNames = Calendar.current.weekdaySymbols

    var body: some View {
        Form {
            Section {
                Toggle("Start downloads on a schedule", isOn: $model.settings.schedule.enabled)
            } footer: {
                Note("Starts the downloads waiting in the list, such as those added with Download Later.")
            }
            Section {
                LabeledContent("Start at") {
                    DatePicker("Start at", selection: time(\.start), displayedComponents: .hourAndMinute).labelsHidden()
                }
                Toggle("Pause them again", isOn: Binding(
                    get: { !model.settings.schedule.stop.isEmpty },
                    set: { model.settings.schedule.stop = $0 ? "08:00" : "" }
                ))
                if !model.settings.schedule.stop.isEmpty {
                    LabeledContent("Pause at") {
                        DatePicker("Pause at", selection: time(\.stop), displayedComponents: .hourAndMinute).labelsHidden()
                    }
                }
                LabeledContent("On") {
                    HStack(spacing: 4) {
                        ForEach(0..<7, id: \.self) { day in
                            Toggle(days[day], isOn: dayBinding(UInt8(day)))
                                .toggleStyle(.button)
                                .help(dayNames[day])
                                .frame(minWidth: 28)
                        }
                    }
                }
            }
            .disabled(!model.settings.schedule.enabled)
        }
        .formStyle(.grouped)
        .frame(width: tabWidth)
        .fixedSize(horizontal: false, vertical: true)
    }

    /// "HH:MM" as a date today, for the time pickers.
    private func time(_ path: WritableKeyPath<Schedule, String>) -> Binding<Date> {
        Binding(
            get: {
                let parts = model.settings.schedule[keyPath: path].split(separator: ":").compactMap { Int($0) }
                return Calendar.current.date(bySettingHour: parts.first ?? 0, minute: parts.dropFirst().first ?? 0, second: 0, of: Date()) ?? Date()
            },
            set: { date in
                let parts = Calendar.current.dateComponents([.hour, .minute], from: date)
                model.settings.schedule[keyPath: path] = String(format: "%02d:%02d", parts.hour ?? 0, parts.minute ?? 0)
            }
        )
    }

    private func dayBinding(_ day: UInt8) -> Binding<Bool> {
        Binding(
            get: { model.settings.schedule.days.contains(day) },
            set: { on in
                var days = Set(model.settings.schedule.days)
                if on { days.insert(day) } else { days.remove(day) }
                model.settings.schedule.days = Data(days.sorted())
            }
        )
    }
}

private struct EngineSettings: View {
    @EnvironmentObject var model: SettingsModel

    var body: some View {
        Form {
            Section {
                EditorBox(text: $model.settings.engineExtra, height: 200, monospaced: true)
            } header: {
                Text("aria2 options")
            } footer: {
                VStack(alignment: .leading, spacing: 6) {
                    Note("One option per line as name=value, for example lowest-speed-limit=10K. They are applied to the running engine, override the same options set elsewhere in Settings, and an option the engine rejects is reported.")
                    Link("All aria2 options", destination: URL(string: "https://aria2.github.io/manual/en/html/aria2c.html#options")!)
                        .font(.callout)
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: tabWidth)
        .fixedSize(horizontal: false, vertical: true)
    }
}
