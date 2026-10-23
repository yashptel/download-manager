import AppKit
import OpenDMCore

/// The menu bar item: total speed while downloading, and quick controls.
@MainActor
final class StatusItemController: NSObject, DownloadStoreObserver {
    private let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    private let store: DownloadStore
    private let speedItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
    private let image = StatusItemController.templateImage(named: "MenuBar")
    private let progressFrames = (0..<GlyphFrame.count).map { StatusItemController.templateImage(named: "MenuBar-Progress-\($0)") }
    private let pausedFrames = (0..<GlyphFrame.count).map { StatusItemController.templateImage(named: "MenuBar-Paused-\($0)") }
    private var displayedFrame: GlyphFrame?
    private var glyphState = GlyphState.idle
    private var completionTimer: Timer?

    init(store: DownloadStore) {
        self.store = store
        super.init()
        if let button = item.button {
            button.image = image
            button.imagePosition = .imageLeading
            button.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        }
        let menu = NSMenu()
        menu.addItem(withTitle: "Show Downloads", action: #selector(AppDelegate.showMainWindow(_:)), keyEquivalent: "")
        menu.addItem(withTitle: "New Download…", action: #selector(AppDelegate.newDownload(_:)), keyEquivalent: "")
        menu.addItem(.separator())
        speedItem.isEnabled = false
        menu.addItem(speedItem)
        menu.addItem(withTitle: "Resume All", action: #selector(AppDelegate.resumeAll(_:)), keyEquivalent: "")
        menu.addItem(withTitle: "Pause All", action: #selector(AppDelegate.pauseAll(_:)), keyEquivalent: "")
        menu.addItem(.separator())
        menu.addItem(withTitle: "Settings…", action: #selector(AppDelegate.showSettings(_:)), keyEquivalent: "")
        menu.addItem(withTitle: "Quit OpenDM", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        item.menu = menu
        store.addObserver(self)
        storeStatusDidChange(store)
    }

    func storeStatusDidChange(_ store: DownloadStore) {
        let stats = store.stats
        item.button?.title = stats.active > 0 ? Format.speed(stats.downloadSpeed) : ""
        updateGlyph()
        let idle = Format.idle(store.rows)
        speedItem.title = switch (stats.active, idle.isEmpty) {
        case (0, true): "No active downloads"
        case (0, false): idle
        case (let active, _) where stats.downloadSpeed > 0: "\(active) downloading · \(Format.speed(stats.downloadSpeed))"
        case (let active, _): "\(active) downloading"
        }
    }

    func store(_ store: DownloadStore, didChange changes: [RowChange]) {
        let completed = changes.contains { $0.old?.status == .downloading && $0.new.status == .complete }
        updateGlyph(completed: completed)
    }

    func store(_ store: DownloadStore, didRemove ids: Set<String>) {
        updateGlyph()
    }

    private func updateGlyph(completed: Bool = false) {
        let next = GlyphState.from(store.rows)
        if completionTimer != nil {
            if next.isDownloading {
                completionTimer?.invalidate()
                completionTimer = nil
            } else {
                return
            }
        }
        if completed && glyphState.isDownloading && !next.isDownloading {
            glyphState = .completed
            show(.completed)
            holdCompletion()
            return
        }
        glyphState = next
        show(next)
    }

    private func holdCompletion() {
        let timer = Timer(timeInterval: 1, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.completionTimer = nil
                self.updateGlyph()
            }
        }
        RunLoop.main.add(timer, forMode: .common)
        completionTimer = timer
    }

    private func show(_ state: GlyphState) {
        let frame = state.frame
        guard frame != displayedFrame else { return }
        displayedFrame = frame
        item.button?.image = frame.progress.map { (frame.dimmed ? pausedFrames : progressFrames)[$0] } ?? image
        showDock(frame)
    }

    /// Without a window OpenDM has no Dock icon, and a new one shows the
    /// plain app icon until it is given the current progress.
    func dockIconDidAppear() {
        if let displayedFrame { showDock(displayedFrame) }
    }

    private func showDock(_ frame: GlyphFrame) {
        if let progress = frame.progress, let image = NSImage(named: "\(frame.dimmed ? "DockPaused" : "DockProgress")-\(progress)") {
            let view = NSImageView(image: image)
            view.imageScaling = .scaleProportionallyUpOrDown
            NSApp.dockTile.contentView = view
        } else {
            NSApp.dockTile.contentView = nil
        }
        NSApp.dockTile.display()
    }

    private static func templateImage(named name: String) -> NSImage? {
        let image = NSImage(named: name)
        image?.isTemplate = true
        image?.accessibilityDescription = "OpenDM"
        return image
    }
}
