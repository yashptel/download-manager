import AppKit
import OpenDMCore

private extension NSToolbarItem.Identifier {
    static let add = Self("add")
    static let resume = Self("resume")
    static let pause = Self("pause")
    static let remove = Self("remove")
    static let search = Self("search")
}

/// The main window: category sidebar, download list and toolbar. Menu and
/// toolbar commands reach it through the responder chain and act on the
/// list's selection.
final class MainWindowController: NSWindowController, NSWindowDelegate, NSToolbarDelegate, NSMenuItemValidation,
    NSToolbarItemValidation, DownloadStoreObserver
{
    private let store: DownloadStore
    private let core: DownloadCore
    private let list: DownloadListViewController

    init(store: DownloadStore, core: DownloadCore) {
        self.store = store
        self.core = core
        list = DownloadListViewController(store: store, core: core)
        let categories = core.settings().categories.map(\.name) + ["Other"]
        let sidebar = SidebarViewController(store: store, categories: categories)

        let split = NSSplitViewController()
        let sidebarItem = NSSplitViewItem(sidebarWithViewController: sidebar)
        sidebarItem.minimumThickness = 170
        sidebarItem.maximumThickness = 280
        split.addSplitViewItem(sidebarItem)
        split.addSplitViewItem(NSSplitViewItem(viewController: list))
        split.splitView.autosaveName = "MainSplit"

        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1080, height: 620),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.title = "OpenDM"
        window.contentViewController = split
        window.minSize = NSSize(width: 640, height: 320)
        window.toolbarStyle = .unified
        window.isReleasedWhenClosed = false
        if !window.setFrameUsingName("MainWindow") {
            window.setContentSize(NSSize(width: 1080, height: 620))
            window.center()
        }
        window.setFrameAutosaveName("MainWindow")
        super.init(window: window)

        let toolbar = NSToolbar(identifier: "Main")
        toolbar.delegate = self
        toolbar.displayMode = .iconOnly
        window.toolbar = toolbar
        window.delegate = self

        sidebar.onSelect = { [weak list] filter in list?.filter = filter }
        store.addObserver(self)
        store.onError = { [weak self] message in self?.showError(message) }
        updateSubtitle()
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    func show() {
        NSApp.setActivationPolicy(.regular)
        showWindow(nil)
        window?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        core.setWindowVisible(visible: true)
    }

    // MARK: Window

    func windowWillClose(_ notification: Notification) {
        core.setWindowVisible(visible: false)
        // Keep running in the menu bar without a Dock icon.
        NSApp.setActivationPolicy(.accessory)
    }

    func windowDidChangeOcclusionState(_ notification: Notification) {
        core.setWindowVisible(visible: window?.occlusionState.contains(.visible) ?? false)
    }

    // MARK: Store

    func storeStatusDidChange(_ store: DownloadStore) {
        updateSubtitle()
        if case .failed(let reason) = store.engineState { showEngineFailure(reason) }
    }

    func store(_ store: DownloadStore, didChange changes: [RowChange]) {
        if changes.contains(where: { $0.old?.status != $0.new.status }) {
            window?.toolbar?.validateVisibleItems()
        }
    }

    private func updateSubtitle() {
        let stats = store.stats
        window?.subtitle = switch store.engineState {
        case .starting: "Starting…"
        case .restarting: "Restarting the download engine…"
        case .failed: "The download engine stopped"
        case .stopped: ""
        case .running where stats.active > 0 && stats.downloadSpeed > 0:
            "\(stats.active) downloading · \(Format.speed(stats.downloadSpeed))"
        case .running where stats.active > 0: "\(stats.active) downloading"
        case .running where stats.waiting > 0: "\(stats.waiting) waiting"
        case .running: ""
        }
    }

    private func showError(_ message: String) {
        guard let window, window.isVisible, window.attachedSheet == nil else {
            NSLog("OpenDM: %@", message)
            return
        }
        let alert = NSAlert()
        alert.messageText = message
        alert.beginSheetModal(for: window)
    }

    private func showEngineFailure(_ reason: String) {
        show()
        guard let window, window.attachedSheet == nil else { return }
        let alert = NSAlert()
        alert.alertStyle = .critical
        alert.messageText = "The download engine keeps stopping"
        alert.informativeText = reason
        alert.addButton(withTitle: "Restart Engine")
        alert.addButton(withTitle: "Close")
        alert.beginSheetModal(for: window) { [core] response in
            if response == .alertFirstButtonReturn { core.restartEngine() }
        }
    }

    // MARK: Commands

    @objc func newDownload(_ sender: Any?) {
        guard let window else { return }
        show()
        AddDownloadPrompt.run(in: window, core: core)
    }

    @objc func paste(_ sender: Any?) {
        newDownload(sender)
    }

    @objc func resumeSelected(_ sender: Any?) { list.resume(list.selectedRows()) }
    @objc func pauseSelected(_ sender: Any?) { list.pause(list.selectedRows()) }
    @objc func removeSelected(_ sender: Any?) { list.remove(list.selectedRows(), deletingFiles: false) }
    @objc func removeSelectedDeletingFiles(_ sender: Any?) { list.remove(list.selectedRows(), deletingFiles: true) }
    @objc func openSelected(_ sender: Any?) { list.open(list.selectedRows()) }
    @objc func revealSelected(_ sender: Any?) { list.reveal(list.selectedRows()) }
    @objc func copy(_ sender: Any?) { list.copyAddresses(list.selectedRows()) }

    @objc private func searchChanged(_ sender: NSSearchField) {
        list.searchText = sender.stringValue
    }

    private func isEnabled(_ action: Selector?) -> Bool {
        let rows = list.selectedRows()
        switch action {
        case #selector(resumeSelected(_:)): return rows.contains { $0.status == .paused || $0.status == .failed }
        case #selector(pauseSelected(_:)): return rows.contains { $0.status == .downloading || $0.status == .queued }
        case #selector(openSelected(_:)): return rows.contains { $0.status == .complete }
        case #selector(removeSelected(_:)), #selector(removeSelectedDeletingFiles(_:)),
             #selector(revealSelected(_:)), #selector(copy(_:)):
            return !rows.isEmpty
        default: return true
        }
    }

    func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        isEnabled(menuItem.action)
    }

    func validateToolbarItem(_ item: NSToolbarItem) -> Bool {
        isEnabled(item.action)
    }

    // MARK: Toolbar

    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [.toggleSidebar, .sidebarTrackingSeparator, .add, .resume, .pause, .remove, .flexibleSpace, .search]
    }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        toolbarDefaultItemIdentifiers(toolbar) + [.space]
    }

    func toolbar(_ toolbar: NSToolbar, itemForItemIdentifier identifier: NSToolbarItem.Identifier,
                 willBeInsertedIntoToolbar flag: Bool) -> NSToolbarItem? {
        switch identifier {
        case .add: return button(identifier, "New Download", "plus", #selector(newDownload(_:)))
        case .resume: return button(identifier, "Resume", "play.fill", #selector(resumeSelected(_:)))
        case .pause: return button(identifier, "Pause", "pause.fill", #selector(pauseSelected(_:)))
        case .remove: return button(identifier, "Remove", "trash", #selector(removeSelected(_:)))
        case .search:
            let item = NSSearchToolbarItem(itemIdentifier: identifier)
            item.searchField.target = self
            item.searchField.action = #selector(searchChanged(_:))
            item.searchField.sendsSearchStringImmediately = true
            return item
        default: return nil
        }
    }

    private func button(_ identifier: NSToolbarItem.Identifier, _ label: String, _ symbol: String,
                        _ action: Selector) -> NSToolbarItem {
        let item = NSToolbarItem(itemIdentifier: identifier)
        item.label = label
        item.paletteLabel = label
        item.toolTip = label
        item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)
        item.isBordered = true
        item.action = action
        return item
    }
}
