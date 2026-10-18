import AppKit
import OpenDMCore
import SwiftUI

/// Where new downloads go before they start: the Download File Info dialog
/// for one, a list for several, or straight to the engine when the user
/// turned confirmation off. A captured download may already be running
/// (`taskId`): the core started it to hear the server's first answer.
@MainActor
enum NewDownloads {
    /// Set at launch; the dialog follows its running download here.
    static var store: DownloadStore?

    static func present(_ downloads: [NewDownload], core: DownloadCore) {
        guard !downloads.isEmpty else { return }
        guard core.settings().confirmDownloads else {
            let running = downloads.compactMap(\.taskId)
            let added = core.addDownloads(downloads: downloads.filter { $0.taskId == nil }, startPaused: false)
            ProgressWindowController.show(ids: running + added, core: core)
            return
        }
        if downloads.count == 1 {
            DownloadInfoController.show(downloads[0], core: core)
        } else {
            BatchDownloadController.show(downloads, core: core)
        }
    }
}

/// A floating panel that stays until answered, cascaded from the last one.
@MainActor
class ConfirmationPanelController: NSWindowController, NSWindowDelegate {
    private static var open: [ConfirmationPanelController] = []

    init(title: String, content: some View) {
        let hosting = NSHostingController(rootView: content)
        hosting.sizingOptions = .preferredContentSize
        let panel = NSPanel(contentViewController: hosting)
        panel.title = title
        // Non-activating, like Spotlight: it takes typing and Return while
        // the browser that sent the download stays the active app, which
        // macOS would not let a background app take over anyway.
        panel.styleMask = [.titled, .closable, .nonactivatingPanel]
        panel.becomesKeyOnlyIfNeeded = false
        panel.level = .floating
        panel.hidesOnDeactivate = false
        panel.isReleasedWhenClosed = false
        panel.standardWindowButton(.miniaturizeButton)?.isHidden = true
        panel.standardWindowButton(.zoomButton)?.isHidden = true
        super.init(window: panel)
        panel.delegate = self
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    func present() {
        guard let window else { return }
        if let last = Self.open.last?.window {
            window.setFrameTopLeftPoint(last.cascadeTopLeft(from: .zero))
        } else {
            window.center()
        }
        Self.open.append(self)
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
    }

    func windowWillClose(_ notification: Notification) {
        Self.open.removeAll { $0 === self }
    }

    /// Asks for a folder in a sheet on this panel.
    func chooseFolder(from current: String, then use: @escaping (String) -> Void) {
        guard let window else { return }
        let picker = NSOpenPanel()
        picker.canChooseDirectories = true
        picker.canChooseFiles = false
        picker.canCreateDirectories = true
        picker.prompt = "Choose"
        picker.directoryURL = URL(fileURLWithPath: current)
        picker.beginSheetModal(for: window) { response in
            if response == .OK, let url = picker.url { use(url.path) }
        }
    }
}

private extension NSWindow {
    func cascadeTopLeft(from point: NSPoint) -> NSPoint {
        NSPoint(x: frame.minX + 24, y: frame.maxY - 24)
    }
}

/// "/Users/me/Downloads/Video" as "~/Downloads/Video".
func abbreviatedPath(_ path: String) -> String {
    (path as NSString).abbreviatingWithTildeInPath
}

// MARK: - One download

@MainActor
final class DownloadInfoModel: ObservableObject {
    let download: NewDownload
    let categories: [String]
    private let core: DownloadCore

    @Published var name: String
    @Published var category: String
    @Published var folder: String
    @Published var size: UInt64?
    @Published var checking = true
    @Published var problem: String?
    @Published var rememberFolder = false
    /// The download, already running while the dialog asks about it, as IDM
    /// does: the server is asked once. nil for streams and magnet links,
    /// which start only when confirmed.
    private(set) var taskId: String?
    /// What the user changed stays as they set it when the server answers.
    private(set) var nameEdited = false
    private(set) var categoryEdited = false
    private(set) var folderChosen = false

    init(download: NewDownload, core: DownloadCore) {
        self.download = download
        self.core = core
        let settings = core.settings()
        let plan = core.planDownload(download: download)
        var categories = settings.categories.map(\.name)
        categories.append("Other")
        if !categories.contains(plan.category) { categories.append(plan.category) }
        self.categories = categories
        name = plan.name
        category = plan.category
        folder = plan.dir
        size = plan.size
        let scheme = download.url.lowercased()
        if let running = download.taskId {
            taskId = running
        } else if download.mediaKind == nil, ["http://", "https://", "ftp://"].contains(where: scheme.hasPrefix) {
            taskId = core.addDownloads(downloads: [download], startPaused: false).first
        }
        checking = taskId != nil
        if let taskId, let store = NewDownloads.store {
            store.addObserver(self)
            if let row = store.row(taskId) { show(row) }
        }
    }

    func setName(_ value: String) {
        name = value
        nameEdited = true
    }

    func setCategory(_ value: String) {
        category = value
        categoryEdited = true
        if !folderChosen { folder = core.categoryDir(category: value) }
    }

    func setFolder(_ value: String) {
        folder = value
        folderChosen = true
    }

    /// What the running download has learned: the server's name and size,
    /// or why it failed.
    fileprivate func show(_ row: TaskRow) {
        if !nameEdited { name = row.name }
        if !categoryEdited, !folderChosen, row.category != category {
            category = row.category
            folder = core.categoryDir(category: row.category)
        }
        if row.totalBytes > 0 { size = row.totalBytes }
        problem = row.status == .failed ? (row.error ?? "The download failed") : nil
        checking = row.status != .failed && row.status != .complete && row.totalBytes == 0 && row.doneBytes == 0
    }

    /// Sends the running download to the name and folder now in the dialog.
    /// It moves there when it completes.
    func applyDestination() {
        guard let taskId, let row = NewDownloads.store?.row(taskId) else { return }
        let typed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let newName = nameEdited && !typed.isEmpty && typed != row.name ? typed : nil
        let newFolder = folder != row.dir ? folder : nil
        if newName != nil || newFolder != nil {
            core.setDestination(id: taskId, dir: newFolder, name: newName)
        }
    }

    var summary: String {
        if checking { return "Checking…" }
        if let problem { return problem }
        if let size, size > 0 { return Format.bytes(size) }
        return download.mediaKind == nil ? "Size unknown" : "Size known once downloaded"
    }

    /// The download as the dialog now describes it.
    var confirmed: NewDownload {
        var download = download
        if nameEdited {
            download.filename = name.trimmingCharacters(in: .whitespacesAndNewlines)
            download.filenameSource = "user"
        }
        download.category = category
        download.dir = folder
        return download
    }

    func saveFolderForCategory() {
        guard rememberFolder else { return }
        var settings = core.settings()
        guard let index = settings.categories.firstIndex(where: { $0.name == category }) else { return }
        settings.categories[index].folder = folder
        try? core.setSettings(settings: settings)
    }
}

/// IDM's Download File Info: confirms one new download before it starts.
@MainActor
final class DownloadInfoController: ConfirmationPanelController {
    private let model: DownloadInfoModel
    private let core: DownloadCore
    private var answered = false

    static func show(_ download: NewDownload, core: DownloadCore) {
        DownloadInfoController(download: download, core: core).present()
    }

    private init(download: NewDownload, core: DownloadCore) {
        let model = DownloadInfoModel(download: download, core: core)
        self.model = model
        self.core = core
        weak var controller: DownloadInfoController?
        let view = DownloadInfoView(
            model: model,
            chooseFolder: { controller?.chooseFolder(from: model.folder) { model.setFolder($0) } },
            finish: { controller?.finish($0) }
        )
        super.init(title: "Download File Info", content: view)
        controller = self
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    private func finish(_ answer: DownloadInfoView.Answer) {
        answered = true
        if answer != .cancel { model.saveFolderForCategory() }
        if let id = model.taskId {
            switch answer {
            case .cancel:
                core.remove(ids: [id], deleteFiles: true)
            case .later:
                core.pause(ids: [id])
                model.applyDestination()
            case .start:
                model.applyDestination()
                if model.problem != nil { core.resume(ids: [id]) }
                ProgressWindowController.show(ids: [id], core: core)
            }
        } else if answer != .cancel {
            let ids = core.addDownloads(downloads: [model.confirmed], startPaused: answer == .later)
            if answer == .start { ProgressWindowController.show(ids: ids, core: core) }
        }
        close()
    }

    /// The close button is Cancel.
    override func windowWillClose(_ notification: Notification) {
        if !answered, let id = model.taskId {
            answered = true
            core.remove(ids: [id], deleteFiles: true)
        }
        super.windowWillClose(notification)
    }
}

struct DownloadInfoView: View {
    enum Answer { case start, later, cancel }

    @ObservedObject var model: DownloadInfoModel
    let chooseFolder: () -> Void
    let finish: (Answer) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(spacing: 14) {
                Image(nsImage: FileIcons.icon(for: model.name, size: 56))
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 3) {
                    Text(model.name)
                        .font(.headline)
                        .lineLimit(2)
                        .truncationMode(.middle)
                    HStack(spacing: 6) {
                        if model.checking { ProgressView().controlSize(.mini) }
                        Text(model.summary)
                            .foregroundStyle(model.problem == nil ? Color.secondary : Color.red)
                    }
                    .font(.callout)
                }
            }

            Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 10, verticalSpacing: 10) {
                GridRow {
                    Text("Name:").gridColumnAlignment(.trailing)
                    TextField("Name", text: Binding(get: { model.name }, set: model.setName))
                        .labelsHidden()
                }
                GridRow {
                    Text("Category:")
                    Picker("Category", selection: Binding(get: { model.category }, set: model.setCategory)) {
                        ForEach(model.categories, id: \.self) { Text($0).tag($0) }
                    }
                    .labelsHidden()
                    .fixedSize()
                }
                GridRow {
                    Text("Save to:")
                    HStack {
                        Text(abbreviatedPath(model.folder))
                            .lineLimit(1)
                            .truncationMode(.head)
                            .help(model.folder)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        Button("Choose…", action: chooseFolder)
                    }
                }
                if model.folderChosen, model.category != "Other" {
                    GridRow {
                        Color.clear.gridCellUnsizedAxes([.horizontal, .vertical])
                        Toggle("Always save \(model.category) here", isOn: $model.rememberFolder)
                    }
                }
                if let quality = model.download.mediaQuality {
                    GridRow {
                        Text("Quality:")
                        Text(quality)
                    }
                }
                GridRow {
                    Text("Address:")
                    Text(model.download.url)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .textSelection(.enabled)
                        .help(model.download.url)
                }
            }

            HStack {
                Spacer()
                Button("Cancel") { finish(.cancel) }
                    .keyboardShortcut(.cancelAction)
                Button("Download Later") { finish(.later) }
                Button("Start Download") { finish(.start) }
                    .keyboardShortcut(.defaultAction)
            }
            .padding(.top, 6)
        }
        .padding(20)
        .frame(width: 540)
    }
}

// MARK: - Several downloads

@MainActor
final class BatchDownloadModel: ObservableObject {
    struct Item: Identifiable {
        let id: Int
        let download: NewDownload
        let plan: FileInfo
        var chosen = true
    }

    @Published var items: [Item]
    /// nil: each file goes to its category's folder.
    @Published var folder: String?

    init(downloads: [NewDownload], core: DownloadCore) {
        items = downloads.enumerated().map { index, download in
            Item(id: index, download: download, plan: core.planDownload(download: download))
        }
    }

    var chosenCount: Int { items.filter(\.chosen).count }

    func setAll(_ on: Bool) {
        for index in items.indices { items[index].chosen = on }
    }

    var confirmed: [NewDownload] {
        items.filter(\.chosen).map { item in
            var download = item.download
            if let folder { download.dir = folder }
            return download
        }
    }
}

/// Confirms several downloads at once, such as all links on a page.
@MainActor
final class BatchDownloadController: ConfirmationPanelController {
    private let model: BatchDownloadModel
    private let core: DownloadCore

    static func show(_ downloads: [NewDownload], core: DownloadCore) {
        BatchDownloadController(downloads: downloads, core: core).present()
    }

    private init(downloads: [NewDownload], core: DownloadCore) {
        let model = BatchDownloadModel(downloads: downloads, core: core)
        self.model = model
        self.core = core
        weak var controller: BatchDownloadController?
        let defaultFolder = core.settings().downloadDir
        let view = BatchDownloadView(
            model: model,
            chooseFolder: { controller?.chooseFolder(from: model.folder ?? defaultFolder) { model.folder = $0 } },
            finish: { controller?.finish($0) }
        )
        super.init(title: "Download \(downloads.count) Files", content: view)
        controller = self
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    private func finish(_ answer: DownloadInfoView.Answer) {
        if answer != .cancel {
            let ids = core.addDownloads(downloads: model.confirmed, startPaused: answer == .later)
            if answer == .start { ProgressWindowController.show(ids: ids, core: core) }
        }
        close()
    }
}

struct BatchDownloadView: View {
    @ObservedObject var model: BatchDownloadModel
    let chooseFolder: () -> Void
    let finish: (DownloadInfoView.Answer) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text("\(model.chosenCount) of \(model.items.count) files selected")
                    .foregroundStyle(.secondary)
                Spacer()
                Button("Select All") { model.setAll(true) }
                Button("Select None") { model.setAll(false) }
            }
            List($model.items) { $item in
                HStack(spacing: 10) {
                    Toggle(isOn: $item.chosen) { EmptyView() }
                        .labelsHidden()
                    Image(nsImage: FileIcons.icon(for: item.plan.name, size: 28))
                        .accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(item.plan.name).lineLimit(1).truncationMode(.middle)
                        Text(item.download.url)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                    Spacer()
                    Text(item.plan.category).foregroundStyle(.secondary)
                }
                .padding(.vertical, 2)
                .help(item.download.url)
            }
            // Plain rows: alternating stripes would not match the taller rows
            // in the empty space below them.
            .listStyle(.bordered(alternatesRowBackgrounds: false))
            .frame(height: 300)
            HStack {
                Text("Save to:")
                Text(model.folder.map(abbreviatedPath) ?? "Each file's category folder")
                    .lineLimit(1)
                    .truncationMode(.head)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if model.folder != nil {
                    Button("Use Category Folders") { model.folder = nil }
                }
                Button("Choose…", action: chooseFolder)
            }
            HStack {
                Spacer()
                Button("Cancel") { finish(.cancel) }
                    .keyboardShortcut(.cancelAction)
                Button("Download Later") { finish(.later) }
                    .disabled(model.chosenCount == 0)
                Button("Start Download") { finish(.start) }
                    .keyboardShortcut(.defaultAction)
                    .disabled(model.chosenCount == 0)
            }
            .padding(.top, 10)
        }
        .padding(20)
        .frame(width: 620)
    }
}

extension DownloadInfoModel: DownloadStoreObserver {
    func store(_ store: DownloadStore, didChange changes: [RowChange]) {
        guard let taskId, let change = changes.last(where: { $0.new.id == taskId }) else { return }
        show(change.new)
    }
}
