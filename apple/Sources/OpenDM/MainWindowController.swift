import AppKit
import OpenDMCore

private extension NSToolbarItem.Identifier {
    static let add = Self("add")
    static let resume = Self("resume")
    static let pause = Self("pause")
    static let remove = Self("remove")
    static let search = Self("search")
    static let style = Self("style")
    static let sort = Self("sort")
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
    private weak var styleGroup: NSToolbarItemGroup?
    /// The bottom status bar on macOS 26 and later; older systems use the
    /// window subtitle instead.
    private var statusLabel: NSTextField?
    private let downloadDir: URL
    private var shown = false

    private static let sortKeys: [(title: String, key: String)] = [
        ("Date Added", "added"), ("Name", "name"), ("Size", "size"), ("Progress", "progress"),
        ("Speed", "speed"), ("Time Left", "eta"), ("Status", "status"),
    ]

    init(store: DownloadStore, core: DownloadCore) {
        self.store = store
        self.core = core
        list = DownloadListViewController(store: store, core: core)
        downloadDir = URL(fileURLWithPath: core.settings().downloadDir)
        let categories = core.settings().categories.map(\.name) + ["Other"]
        let sidebar = SidebarViewController(store: store, categories: categories)

        let split = NSSplitViewController()
        let sidebarItem = NSSplitViewItem(sidebarWithViewController: sidebar)
        sidebarItem.minimumThickness = 210
        sidebarItem.maximumThickness = 280
        split.addSplitViewItem(sidebarItem)
        let listItem = NSSplitViewItem(viewController: list)
        if #available(macOS 26, *) {
            // The list extends under the glass sidebar so the glass has
            // content to refract; the scroll view insets rows by the safe area.
            listItem.automaticallyAdjustsSafeAreaInsets = true
            // A Finder-style status bar; the list scrolls under its glass edge.
            let label = NSTextField(labelWithString: "")
            label.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
            label.textColor = .secondaryLabelColor
            label.alignment = .center
            label.lineBreakMode = .byTruncatingTail
            let bar = NSSplitViewItemAccessoryViewController()
            bar.view = label
            listItem.addBottomAlignedAccessoryViewController(bar)
            statusLabel = label
        }
        split.addSplitViewItem(listItem)
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
        ProgressWindowController.openChanged = { [weak self] in self?.reportVisible() }
        updateSubtitle()
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    func show() {
        NSApp.setActivationPolicy(.regular)
        showWindow(nil)
        window?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        shown = true
        reportVisible()
    }

    // MARK: Window

    func windowWillClose(_ notification: Notification) {
        shown = false
        reportVisible()
        // Keep running in the menu bar without a Dock icon.
        NSApp.setActivationPolicy(.accessory)
    }

    func windowDidChangeOcclusionState(_ notification: Notification) {
        shown = window?.occlusionState.contains(.visible) ?? false
        reportVisible()
    }

    /// Progress updates live while this window or a progress window shows it.
    private func reportVisible() {
        core.setWindowVisible(visible: shown || ProgressWindowController.anyOpen)
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
        let status = switch store.engineState {
        case .starting: "Starting…"
        case .restarting: "Restarting the download engine…"
        case .failed: "The download engine stopped"
        case .stopped: ""
        case .running where stats.active > 0 && stats.downloadSpeed > 0:
            "\(stats.active) downloading · \(Format.speed(stats.downloadSpeed))"
        case .running where stats.active > 0: "\(stats.active) downloading"
        case .running: Format.idle(store.rows)
        }
        guard let statusLabel else {
            window?.subtitle = status
            return
        }
        let free = (try? downloadDir.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey]))?
            .volumeAvailableCapacityForImportantUsage
        let space = free.map { "\(Format.bytes(UInt64(max($0, 0)))) available" }
        statusLabel.setIfChanged([status, space].compactMap { $0?.isEmpty == false ? $0 : nil }.joined(separator: " · "))
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

    // MARK: Style and sorting

    @objc func setListStyle(_ sender: NSMenuItem) {
        setStyle(ListStyle(rawValue: sender.tag) ?? .cards)
    }

    @objc private func styleGroupChanged(_ sender: Any?) {
        guard let group = styleGroup, let style = ListStyle(rawValue: group.selectedIndex) else { return }
        setStyle(style)
    }

    private func setStyle(_ style: ListStyle) {
        list.style = style
        styleGroup?.selectedIndex = style.rawValue
    }

    @objc private func sortBy(_ sender: NSMenuItem) {
        guard let key = sender.representedObject as? String else { return }
        let current = list.sort
        let ascending = current?.key == key ? current!.ascending : (key == "name" || key == "added")
        list.setSort(key: key, ascending: ascending)
    }

    @objc private func sortDirection(_ sender: NSMenuItem) {
        list.setSort(key: list.sort?.key ?? "added", ascending: sender.tag == 1)
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
        switch menuItem.action {
        case #selector(setListStyle(_:)):
            menuItem.state = menuItem.tag == list.style.rawValue ? .on : .off
        case #selector(sortBy(_:)):
            menuItem.state = (menuItem.representedObject as? String) == (list.sort?.key ?? "added") ? .on : .off
        case #selector(sortDirection(_:)):
            menuItem.state = (menuItem.tag == 1) == (list.sort?.ascending ?? true) ? .on : .off
        default:
            break
        }
        return isEnabled(menuItem.action)
    }

    func validateToolbarItem(_ item: NSToolbarItem) -> Bool {
        isEnabled(item.action)
    }

    // MARK: Toolbar

    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [.toggleSidebar, .sidebarTrackingSeparator, .add, .resume, .pause, .remove, .flexibleSpace, .style, .sort, .search]
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
        case .style:
            let group = NSToolbarItemGroup(
                itemIdentifier: identifier,
                images: ListStyle.allCases.map { NSImage(systemSymbolName: $0.symbol, accessibilityDescription: $0.title)! },
                selectionMode: .selectOne,
                labels: ListStyle.allCases.map(\.title),
                target: self,
                action: #selector(styleGroupChanged(_:))
            )
            group.label = "View"
            group.paletteLabel = "View"
            group.selectedIndex = list.style.rawValue
            styleGroup = group
            return group
        case .sort:
            let item = NSMenuToolbarItem(itemIdentifier: identifier)
            item.image = NSImage(systemSymbolName: "arrow.up.arrow.down", accessibilityDescription: "Sort")
            item.label = "Sort"
            item.toolTip = "Sort"
            let menu = NSMenu()
            for (title, key) in Self.sortKeys {
                let entry = menu.addItem(withTitle: title, action: #selector(sortBy(_:)), keyEquivalent: "")
                entry.target = self
                entry.representedObject = key
            }
            menu.addItem(.separator())
            for (title, tag) in [("Ascending", 1), ("Descending", 0)] {
                let entry = menu.addItem(withTitle: title, action: #selector(sortDirection(_:)), keyEquivalent: "")
                entry.target = self
                entry.tag = tag
            }
            item.menu = menu
            return item
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
        if #available(macOS 26, *), identifier == .add {
            // The accent-tinted glass button marks the main action.
            item.style = .prominent
        }
        return item
    }
}
