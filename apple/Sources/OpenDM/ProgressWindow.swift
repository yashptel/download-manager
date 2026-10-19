import AppKit
import OpenDMCore
import SwiftUI

/// IDM's download progress window: one small window per running download,
/// with Pause and Cancel, that turns into a "download complete" window.
@MainActor
final class ProgressWindowController: NSWindowController, NSWindowDelegate, DownloadStoreObserver {
    /// Set at launch; the windows follow the store's rows.
    static var store: DownloadStore?
    private static var open: [String: ProgressWindowController] = [:]
    /// Called when a progress window opens or closes.
    static var openChanged: (() -> Void)?
    static var anyOpen: Bool { !open.isEmpty }
    /// More downloads than this started at once (a page of links) get no
    /// windows of their own.
    private static let maxAtOnce = 3

    private let id: String
    private let core: DownloadCore
    private let model: ProgressModel
    private var timer: Timer?
    private var fetching = false

    /// Opens windows for downloads that just started, when the user wants them.
    static func show(ids: [String], core: DownloadCore) {
        guard let store, !ids.isEmpty, ids.count <= maxAtOnce, core.settings().showProgressWindow else { return }
        for id in ids {
            if let existing = open[id] {
                existing.window?.orderFrontRegardless()
                continue
            }
            let controller = ProgressWindowController(id: id, store: store, core: core)
            open[id] = controller
            controller.present()
            openChanged?()
        }
    }

    private init(id: String, store: DownloadStore, core: DownloadCore) {
        self.id = id
        self.core = core
        let model = ProgressModel(row: store.row(id))
        self.model = model
        weak var controller: ProgressWindowController?
        let view = DownloadProgressView(model: model, act: { controller?.perform($0) })
        let hosting = NSHostingController(rootView: view)
        hosting.sizingOptions = .preferredContentSize
        let panel = NSPanel(contentViewController: hosting)
        // Shown without taking focus from the browser the download came from.
        panel.styleMask = [.titled, .closable, .miniaturizable, .nonactivatingPanel]
        panel.hidesOnDeactivate = false
        panel.isReleasedWhenClosed = false
        panel.standardWindowButton(.zoomButton)?.isHidden = true
        super.init(window: panel)
        controller = self
        panel.delegate = self
        store.addObserver(self)
        updateTitle()
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    private func present() {
        guard let window else { return }
        let others = Self.open.values.compactMap(\.window).filter { $0 !== window && $0.isVisible }
        if let last = others.last {
            window.setFrameTopLeftPoint(NSPoint(x: last.frame.minX + 24, y: last.frame.maxY - 24))
        } else {
            window.center()
        }
        window.orderFrontRegardless()
        timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.fetchDetails() }
        }
        fetchDetails()
    }

    func windowWillClose(_ notification: Notification) {
        timer?.invalidate()
        Self.open[id] = nil
        Self.openChanged?()
    }

    // MARK: Store

    func store(_ store: DownloadStore, didChange changes: [RowChange]) {
        guard let change = changes.last(where: { $0.new.id == id }) else { return }
        model.row = change.new
        updateTitle()
    }

    func store(_ store: DownloadStore, didRemove ids: Set<String>) {
        if ids.contains(id) { close() }
    }

    private func updateTitle() {
        guard let row = model.row else {
            window?.title = "Starting…"
            return
        }
        let percent = Format.percent(row)
        window?.title = row.status == .complete ? "Download Complete" : [percent, row.name].filter { !$0.isEmpty }.joined(separator: " ")
    }

    /// The segment map and connections, while the download runs.
    private func fetchDetails() {
        guard !fetching, let row = model.row, row.status == .downloading || model.details == nil else { return }
        fetching = true
        let core = core
        let id = id
        DispatchQueue.global(qos: .utility).async {
            let details = core.taskDetails(id: id, cells: 120)
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    self.fetching = false
                    self.model.details = details
                }
            }
        }
    }

    // MARK: Actions

    private func perform(_ action: DownloadProgressView.Action) {
        guard let row = model.row else { return close() }
        switch action {
        case .pause: core.pause(ids: [id])
        case .resume: core.resume(ids: [id])
        case .cancel:
            // As in IDM: the download stops and stays in the list.
            core.pause(ids: [id])
            close()
        case .open:
            if let path = row.path { NSWorkspace.shared.open(URL(fileURLWithPath: path)) }
            close()
        case .reveal:
            let url = row.path.map { URL(fileURLWithPath: $0) } ?? URL(fileURLWithPath: row.dir)
            NSWorkspace.shared.activateFileViewerSelecting([url])
            close()
        case .close: close()
        }
    }
}

@MainActor
final class ProgressModel: ObservableObject {
    @Published var row: TaskRow?
    @Published var details: TaskDetails?
    @AppStorage("progressDetailsShown") var detailsShown = false

    init(row: TaskRow?) {
        self.row = row
    }
}

struct DownloadProgressView: View {
    enum Action { case pause, resume, cancel, open, reveal, close }

    @ObservedObject var model: ProgressModel
    let act: (Action) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            if let row = model.row {
                header(row)
                if row.status == .complete {
                    complete(row)
                } else {
                    running(row)
                }
            } else {
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text("Starting…").foregroundStyle(.secondary)
                }
            }
        }
        .padding(20)
        .frame(width: 460)
    }

    private func header(_ row: TaskRow) -> some View {
        HStack(spacing: 12) {
            Image(nsImage: FileIcons.icon(for: row.name, size: 40))
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(row.name).font(.headline).lineLimit(1).truncationMode(.middle)
                Text(row.url)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .textSelection(.enabled)
                    .help(row.url)
            }
        }
    }

    @ViewBuilder
    private func running(_ row: TaskRow) -> some View {
        if row.progress < 0 && row.status == .downloading {
            ProgressView().progressViewStyle(.linear)
        } else {
            StatusBar(value: min(max(row.progress, 0), 1), color: Color(nsColor: StatusLook.of(row).color))
        }
        let downloading = row.status == .downloading
        Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 10, verticalSpacing: 6) {
            fact("Status", row.status == .failed ? (row.error ?? "Failed") : Format.status(row))
            fact("Downloaded", [Format.amount(row), Format.percent(row)].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · "))
            // These rows stay while paused so the window keeps its height.
            fact("Speed", downloading ? Format.speed(row.speed) : "", always: true)
            fact("Time left", downloading ? Format.timeLeft(row.etaSecs) : "", always: true)
            fact("Connections", downloading && row.connections > 0 ? "\(row.connections)" : "", always: true)
        }
        .font(.callout)
        DisclosureGroup("Details", isExpanded: $model.detailsShown) {
            details.padding(.top, 6)
        }
        HStack {
            Spacer()
            Button("Cancel") { act(.cancel) }
                .help("Stops the download; it stays in the list to resume later.")
                .keyboardShortcut(.cancelAction)
            if row.status == .paused || row.status == .failed {
                Button(row.status == .failed ? "Retry" : "Resume") { act(.resume) }
                    .keyboardShortcut(.defaultAction)
            } else {
                Button("Pause") { act(.pause) }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(.top, 10)
    }

    @ViewBuilder
    private var details: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let segments = model.details?.segments, !segments.isEmpty {
                SegmentBar(segments: [UInt8](segments), color: Color(nsColor: model.row.map { StatusLook.of($0).color } ?? .controlAccentColor))
                    .frame(height: 12)
                    .accessibilityLabel("Finished parts of the file")
                    .help("Finished parts of the file. Each connection fills its own part.")
            }
            // The engine reports the servers in use, not each connection.
            let hosts = Array(Set((model.details?.connections ?? []).map(\.host))).sorted()
            if !hosts.isEmpty {
                Text((hosts.count == 1 ? "Server: " : "Servers: ") + hosts.joined(separator: ", "))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder
    private func complete(_ row: TaskRow) -> some View {
        Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 10, verticalSpacing: 6) {
            fact("Size", row.totalBytes > 0 ? Format.bytes(row.totalBytes) : "")
            fact("Saved to", abbreviatedPath(row.path ?? row.dir))
        }
        .font(.callout)
        HStack {
            Spacer()
            Button("Close") { act(.close) }
                .keyboardShortcut(.cancelAction)
            Button("Show in Finder") { act(.reveal) }
            // Prominent by hand: this panel never becomes key, so its default
            // button would otherwise stay grey.
            Button("Open") { act(.open) }
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
        }
        .padding(.top, 10)
    }

    @ViewBuilder
    private func fact(_ label: String, _ value: String, always: Bool = false) -> some View {
        if !value.isEmpty || always {
            GridRow {
                Text(label).foregroundStyle(.secondary).gridColumnAlignment(.trailing)
                Text(value.isEmpty ? "—" : value)
                    .monospacedDigit()
                    .lineLimit(2)
                    .foregroundStyle(value.isEmpty ? .secondary : .primary)
            }
        }
    }
}

/// A determinate bar in the status color. Drawn by hand because the system
/// bar turns grey in a panel that never becomes key, as this one never does.
struct StatusBar: View {
    let value: Double
    let color: Color

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                Capsule().fill(.secondary.opacity(0.2))
                if value > 0 {
                    Capsule()
                        .fill(color)
                        .frame(width: max(geometry.size.height, geometry.size.width * value))
                }
            }
        }
        .frame(height: 6)
        .accessibilityElement()
        .accessibilityValue(Text("\(Int(value * 100)) percent"))
    }
}

/// The file in equal parts, each filled as far as it is downloaded.
struct SegmentBar: View {
    let segments: [UInt8]
    /// The progress bar's color, so a paused download's parts turn with it.
    let color: Color

    var body: some View {
        Canvas { context, size in
            let width = size.width / CGFloat(segments.count)
            context.fill(Path(roundedRect: CGRect(origin: .zero, size: size), cornerRadius: 3), with: .color(.secondary.opacity(0.15)))
            for (index, done) in segments.enumerated() where done > 0 {
                let rect = CGRect(x: CGFloat(index) * width, y: 0, width: width + 0.5, height: size.height)
                context.fill(Path(rect), with: .color(color.opacity(0.25 + 0.75 * Double(done) / 100)))
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 3))
    }
}
