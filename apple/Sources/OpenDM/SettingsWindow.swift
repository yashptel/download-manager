import AppKit
import OpenDMCore
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
        NSApp.activate(ignoringOtherApps: true)
    }

    private init(core: DownloadCore) {
        let model = SettingsModel(core: core)
        let tabs = NSTabViewController()
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
    }

    private func save() {
        do {
            try core.setSettings(settings: settings)
        } catch {
            NSAlert(error: error).runModal()
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
            HStack {
                Text(abbreviatedPath(path)).lineLimit(1).truncationMode(.head).help(path)
                Button("Choose…") { model.chooseFolder(from: path) { path = $0 } }
            }
        }
    }
}

private struct Note: View {
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
    }
}

// MARK: - Tabs

private struct GeneralSettings: View {
    @EnvironmentObject var model: SettingsModel

    var body: some View {
        Form {
            Section {
                FolderRow(title: "Save downloads to", path: $model.settings.downloadDir)
                Toggle("Sort into category folders", isOn: $model.settings.categoryFolders)
                Stepper("Downloads at a time: \(model.settings.maxConcurrentDownloads)",
                        value: model.number(\.maxConcurrentDownloads), in: 1...20)
            }
            Section("When a download is added") {
                Toggle("Ask before starting it", isOn: $model.settings.confirmDownloads)
                Note("Shows the Download File Info dialog to name the file and pick its folder. Turned off, downloads start at once.")
                Toggle("Show its progress window", isOn: $model.settings.showProgressWindow)
            }
        }
        .formStyle(.grouped)
        .frame(width: 560)
        .fixedSize(horizontal: false, vertical: true)
    }
}

private struct BrowserSettings: View {
    @EnvironmentObject var model: SettingsModel

    var body: some View {
        Form {
            Section {
                Toggle("Take over downloads from the browser", isOn: $model.settings.browserCapture)
                Note("Needs the OpenDM Connect extension. Alt-click a link to keep that download in the browser.")
            }
            Section("File types to take over") {
                TextEditor(text: model.words(\.captureExtensions)).frame(height: 80)
            }
            Section("Never take downloads from these sites") {
                TextEditor(text: model.words(\.captureExcludedSites, separator: "\n")).frame(height: 60)
                Note("One per line. A site also covers its subdomains.")
            }
            Section {
                LabeledContent("Leave files smaller than") {
                    HStack {
                        TextField("", value: Binding(get: { Int(model.settings.captureMinSize / 1_000_000) },
                                                     set: { model.settings.captureMinSize = UInt64(max($0, 0)) * 1_000_000 }),
                                  format: .number)
                            .frame(width: 70)
                            .multilineTextAlignment(.trailing)
                        Text("MB")
                    }
                }
                Note("0 takes files of any size.")
            }
        }
        .formStyle(.grouped)
        .frame(width: 560)
        .fixedSize(horizontal: false, vertical: true)
        .disabled(false)
    }
}

private struct CategorySettings: View {
    @EnvironmentObject var model: SettingsModel
    @State private var selection: Int?

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("Category").frame(width: 130, alignment: .leading)
                Text("File types")
                Spacer()
                Text("Folder").frame(width: 200, alignment: .leading)
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 8)
            List(selection: $selection) {
                ForEach(model.settings.categories.indices, id: \.self) { index in
                    CategoryRow(rule: $model.settings.categories[index], downloadDir: model.settings.downloadDir)
                        .tag(index)
                }
            }
            .listStyle(.bordered(alternatesRowBackgrounds: true))
            .frame(height: 240)
            HStack(spacing: 8) {
                Button { addCategory() } label: { Image(systemName: "plus") }
                    .help("Add a category")
                Button { removeCategory() } label: { Image(systemName: "minus") }
                    .help("Remove the selected category")
                    .disabled(selection == nil)
                Spacer()
            }
            .buttonStyle(.borderless)
            Note("A file goes to the first category that lists its extension; the rest go to Other. A folder name without a path is inside the download folder.")
        }
        .padding(20)
        .frame(width: 660)
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
        HStack {
            TextField("Name", text: $rule.name)
                .labelsHidden()
                .frame(width: 130)
            TextField("Extensions", text: extensions)
                .labelsHidden()
            Text(rule.folder)
                .lineLimit(1)
                .truncationMode(.head)
                .help(rule.folder)
                .frame(width: 130, alignment: .leading)
            Button("Choose…") {
                let current = rule.folder.hasPrefix("/") ? rule.folder : (downloadDir as NSString).appendingPathComponent(rule.folder)
                model.chooseFolder(from: current) { rule.folder = $0 }
            }
            .controlSize(.small)
        }
    }
}

private struct ConnectionSettings: View {
    @EnvironmentObject var model: SettingsModel

    var body: some View {
        Form {
            Section("Speed") {
                speedRow("All downloads together", \.maxDownloadSpeed)
                speedRow("Each download", \.maxTaskSpeed)
            }
            Section("Connections") {
                Stepper("Connections per server: \(model.settings.maxConnectionsPerServer)", value: model.number(\.maxConnectionsPerServer), in: 1...16)
                Stepper("Split each file into: \(model.settings.split) parts", value: model.number(\.split), in: 1...64)
                LabeledContent("Smallest part") {
                    TextField("", text: $model.settings.minSplitSize).frame(width: 70).multilineTextAlignment(.trailing)
                }
                Note("Sizes such as 1M or 512K. Files smaller than two parts download in one piece.")
            }
            Section("Retries") {
                numberRow("Tries per server", \.maxTries, unit: "0 keeps trying")
                numberRow("Wait between tries", \.retryWait, unit: "seconds")
                numberRow("Give up on a silent connection after", \.timeout, unit: "seconds")
                numberRow("Give up connecting after", \.connectTimeout, unit: "seconds")
            }
            Section("Proxy") {
                TextField("Proxy", text: $model.settings.proxy, prompt: Text("http://user:password@host:8080"))
                TextField("Not for", text: $model.settings.noProxy, prompt: Text("localhost, .example.com"))
            }
            Section("Requests") {
                TextField("User agent", text: $model.settings.userAgent, prompt: Text("The engine's own"))
                Note("For downloads you add yourself; the browser's go with the browser's user agent.")
                LabeledContent("Headers for every download") {
                    TextEditor(text: model.words(\.customHeaders, separator: "\n"))
                        .font(.body.monospaced())
                        .frame(height: 54)
                }
                Note("One \"Name: value\" per line.")
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
        .frame(width: 560, height: 620)
    }

    private func speedRow(_ title: String, _ path: WritableKeyPath<AppSettings, UInt64>) -> some View {
        LabeledContent(title) {
            HStack {
                TextField("", value: model.speed(path), format: .number).frame(width: 80).multilineTextAlignment(.trailing)
                Text("KB/s")
                Text(model.settings[keyPath: path] == 0 ? "no limit" : "").foregroundStyle(.secondary).frame(width: 52, alignment: .leading)
            }
        }
    }

    private func numberRow(_ title: String, _ path: WritableKeyPath<AppSettings, UInt32>, unit: String) -> some View {
        LabeledContent(title) {
            HStack {
                TextField("", value: model.number(path), format: .number).frame(width: 60).multilineTextAlignment(.trailing)
                Text(unit).foregroundStyle(.secondary)
            }
        }
    }
}

private struct SchedulerSettings: View {
    @EnvironmentObject var model: SettingsModel
    private let days = Calendar.current.veryShortWeekdaySymbols

    var body: some View {
        Form {
            Section {
                Toggle("Start downloads on a schedule", isOn: $model.settings.schedule.enabled)
                Note("Starts the downloads waiting in the list, such as those added with Download Later.")
            }
            Section {
                DatePicker("Start at", selection: time(\.start), displayedComponents: .hourAndMinute)
                Toggle("Pause them again", isOn: Binding(
                    get: { !model.settings.schedule.stop.isEmpty },
                    set: { model.settings.schedule.stop = $0 ? "08:00" : "" }
                ))
                if !model.settings.schedule.stop.isEmpty {
                    DatePicker("Pause at", selection: time(\.stop), displayedComponents: .hourAndMinute)
                }
                LabeledContent("On") {
                    HStack(spacing: 4) {
                        ForEach(0..<7, id: \.self) { day in
                            Toggle(days[day], isOn: dayBinding(UInt8(day)))
                                .toggleStyle(.button)
                        }
                    }
                }
            }
            .disabled(!model.settings.schedule.enabled)
        }
        .formStyle(.grouped)
        .frame(width: 560)
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
        VStack(alignment: .leading, spacing: 10) {
            Text("aria2 options")
                .font(.headline)
            Note("One option per line as name=value, for example lowest-speed-limit=10K. They are applied to the running engine; an option it rejects is reported. They override the same options set elsewhere in Settings.")
            TextEditor(text: $model.settings.engineExtra)
                .font(.body.monospaced())
                .frame(height: 220)
                .overlay(RoundedRectangle(cornerRadius: 4).stroke(.separator))
            Link("All aria2 options", destination: URL(string: "https://aria2.github.io/manual/en/html/aria2c.html#options")!)
        }
        .padding(20)
        .frame(width: 560)
    }
}
