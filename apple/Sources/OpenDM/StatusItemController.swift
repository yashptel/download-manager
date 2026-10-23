import AppKit
import OpenDMCore

/// The menu bar item: total speed while downloading, and quick controls.
@MainActor
final class StatusItemController: NSObject, DownloadStoreObserver {
    private let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    private let store: DownloadStore
    private let speedItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
    private let image = StatusItemController.templateImage(named: "MenuBar")
    /// While downloading, the icon's chunks scroll down through the arrow.
    private let downloadingFrames = (0..<7).map { StatusItemController.templateImage(named: "MenuBar-Downloading-\($0)") }
    private var animation: Timer?
    private var frame = 0

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
        if stats.active > 0 && !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            startAnimation()
        } else {
            stopAnimation()
        }
        let idle = Format.idle(store.rows)
        speedItem.title = switch (stats.active, idle.isEmpty) {
        case (0, true): "No active downloads"
        case (0, false): idle
        case (let active, _) where stats.downloadSpeed > 0: "\(active) downloading · \(Format.speed(stats.downloadSpeed))"
        case (let active, _): "\(active) downloading"
        }
    }

    private func startAnimation() {
        guard animation == nil else { return }
        let timer = Timer(timeInterval: 0.16, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.advanceFrame() }
        }
        // The common modes keep it running while the menu is open.
        RunLoop.main.add(timer, forMode: .common)
        animation = timer
    }

    private func stopAnimation() {
        guard let animation else { return }
        animation.invalidate()
        self.animation = nil
        frame = 0
        item.button?.image = image
    }

    private func advanceFrame() {
        frame = (frame + 1) % downloadingFrames.count
        item.button?.image = downloadingFrames[frame]
    }

    private static func templateImage(named name: String) -> NSImage? {
        let image = NSImage(named: name)
        image?.isTemplate = true
        image?.accessibilityDescription = "OpenDM"
        return image
    }
}
