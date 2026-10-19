import AppKit
import OpenDMCore
import os

enum ListFilter: Hashable {
    case all, unfinished, finished
    case category(String)
}

private enum Column: String, CaseIterable {
    case name, size, progress, speed, eta, status, added
    /// The single column of the Cards and List styles.
    case main

    var title: String {
        switch self {
        case .name: "Name"
        case .size: "Size"
        case .progress: "Progress"
        case .speed: "Speed"
        case .eta: "Time Left"
        case .status: "Status"
        case .added: "Added"
        case .main: ""
        }
    }

    var width: CGFloat {
        switch self {
        case .name: 260
        case .progress: 110
        case .status: 130
        case .added: 110
        case .main: 600
        default: 76
        }
    }

    var alignment: NSTextAlignment {
        switch self {
        case .size, .speed, .eta: .right
        default: .left
        }
    }

    var identifier: NSUserInterfaceItemIdentifier { .init(rawValue) }
}

private let log = Logger(subsystem: "org.opendm.OpenDM", category: "list")

/// The download table. Rows are looked up by id in the store; `ids` is the
/// filtered, sorted view. A progress tick redraws only the visible cells of
/// changed rows; only filter or sort changes rebuild the view.
final class DownloadListViewController: NSViewController, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate,
    DownloadStoreObserver
{
    private let store: DownloadStore
    private let core: DownloadCore
    private let tableView = DownloadTableView()
    private let emptyView = NSStackView()
    private let emptyTitle = NSTextField(labelWithString: "")
    private let emptySubtitle = NSTextField(labelWithString: "")
    private var headerView: NSTableHeaderView?
    private var ids: [String] = []
    private var positionByID: [String: Int] = [:]

    var filter: ListFilter = .all {
        didSet { if filter != oldValue { rebuild() } }
    }

    var searchText = "" {
        didSet { if searchText != oldValue { rebuild() } }
    }

    var style: ListStyle = .saved {
        didSet {
            guard style != oldValue else { return }
            ListStyle.saved = style
            applyStyle()
        }
    }

    init(store: DownloadStore, core: DownloadCore) {
        self.store = store
        self.core = core
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    override func loadView() {
        for column in Column.allCases {
            let tableColumn = NSTableColumn(identifier: column.identifier)
            tableColumn.title = column.title
            tableColumn.width = column.width
            tableColumn.minWidth = column == .name ? 120 : 40
            tableColumn.headerCell.alignment = column.alignment
            if column != .main {
                tableColumn.sortDescriptorPrototype = NSSortDescriptor(key: column.rawValue, ascending: column == .name)
            }
            tableView.addTableColumn(tableColumn)
        }
        tableView.style = .inset
        tableView.allowsMultipleSelection = true
        tableView.autosaveName = "DownloadList"
        tableView.dataSource = self
        tableView.delegate = self
        tableView.target = self
        tableView.doubleAction = #selector(openClicked(_:))
        tableView.menu = NSMenu()
        tableView.menu?.delegate = self
        tableView.headerView?.menu = columnMenu()
        headerView = tableView.headerView
        tableView.registerForDraggedTypes([.URL, .string])
        tableView.onKey = { [weak self] event in self?.handleKey(event) ?? false }

        let scrollView = NSScrollView()
        scrollView.documentView = tableView
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true

        let emptySymbol = NSImageView(image: NSImage(systemSymbolName: "arrow.down.circle", accessibilityDescription: nil)!
            .withSymbolConfiguration(.init(pointSize: 44, weight: .ultraLight))!)
        emptySymbol.contentTintColor = .tertiaryLabelColor
        emptyTitle.font = .systemFont(ofSize: 17, weight: .semibold)
        emptyTitle.textColor = .secondaryLabelColor
        emptySubtitle.textColor = .tertiaryLabelColor
        for view in [emptySymbol, emptyTitle, emptySubtitle] {
            emptyView.addArrangedSubview(view)
        }
        emptyView.orientation = .vertical
        emptyView.spacing = 6
        emptyView.setCustomSpacing(12, after: emptySymbol)

        let container = NSView()
        for view in [scrollView, emptyView] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            container.addSubview(view)
        }
        NSLayoutConstraint.activate([
            scrollView.topAnchor.constraint(equalTo: container.topAnchor),
            scrollView.bottomAnchor.constraint(equalTo: container.bottomAnchor),
            scrollView.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            scrollView.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            // The safe area excludes the sidebar the list extends under.
            emptyView.centerXAnchor.constraint(equalTo: container.safeAreaLayoutGuide.centerXAnchor),
            emptyView.centerYAnchor.constraint(equalTo: container.safeAreaLayoutGuide.centerYAnchor, constant: -20),
        ])
        view = container
        applyStyle()
    }

    /// Switches between the Cards, List and Columns designs in place.
    private func applyStyle() {
        let started = DispatchTime.now()
        defer {
            let milliseconds = Double(DispatchTime.now().uptimeNanoseconds - started.uptimeNanoseconds) / 1_000_000
            log.debug("applied style \(self.style.title, privacy: .public) in \(milliseconds, format: .fixed(precision: 2)) ms")
        }
        tableView.columnAutoresizingStyle = .noColumnAutoresizing
        for tableColumn in tableView.tableColumns {
            let column = Column(rawValue: tableColumn.identifier.rawValue)
            tableColumn.isHidden = style == .columns ? (column == .main || column == .added) : column != .main
            if style == .columns, let column { tableColumn.width = column.width }
        }
        tableView.headerView = style == .columns ? headerView : nil
        tableView.rowHeight = style.rowHeight
        // The inset style's default 17 pt gap is too wide between six columns.
        tableView.intercellSpacing = NSSize(width: style == .columns ? 10 : 17, height: 0)
        tableView.backgroundColor = style == .cards ? .windowBackgroundColor : .controlBackgroundColor
        tableView.reloadData()
        fitColumns()
        rebuild()
    }

    /// Columns: Name takes whatever the fixed columns leave. Cards and List:
    /// the one column fills the table. Runs again on every layout pass, so
    /// window resizes also go to the first visible column.
    private func fitColumns() {
        guard let scrollView = tableView.enclosingScrollView else { return }
        // Content insets cover the sidebar when the list extends under it.
        let insets = scrollView.contentInsets
        let clipWidth = scrollView.frame.width - insets.left - insets.right
        guard clipWidth > 0 else { return }
        let visible = tableView.tableColumns.filter { !$0.isHidden }
        guard let flexible = visible.first else { return }
        let spacing = tableView.intercellSpacing.width * CGFloat(visible.count)
        let inset: CGFloat = tableView.style == .inset ? 20 : 0
        let fixed = visible.dropFirst().reduce(0) { $0 + $1.width }
        let width = max(flexible.minWidth, clipWidth - fixed - spacing - inset)
        if abs(flexible.width - width) > 0.5 { flexible.width = width }
    }

    override func viewDidLayout() {
        super.viewDidLayout()
        fitColumns()
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        store.addObserver(self)
        rebuild()
    }

    // MARK: Store updates

    func store(_ store: DownloadStore, didChange changes: [RowChange]) {
        let started = DispatchTime.now()
        var changedPositions: [Int] = []
        for change in changes {
            let shown = positionByID[change.new.id]
            if (shown != nil) != matches(change.new) || change.old.map({ sortValueChanged($0, change.new) }) == true {
                rebuild()
                logDuration(since: started, rows: changes.count, rebuilt: true)
                return
            }
            if let shown { changedPositions.append(shown) }
        }
        refresh(changedPositions)
        logDuration(since: started, rows: changes.count, rebuilt: false)
    }

    func store(_ store: DownloadStore, didRemove ids: Set<String>) {
        rebuild()
    }

    private func logDuration(since start: DispatchTime, rows: Int, rebuilt: Bool) {
        let milliseconds = Double(DispatchTime.now().uptimeNanoseconds - start.uptimeNanoseconds) / 1_000_000
        log.debug("applied \(rows) rows in \(milliseconds, format: .fixed(precision: 2)) ms rebuilt=\(rebuilt)")
    }

    /// Reconfigures the on-screen cells of the given rows in place. A row
    /// that now shows a different download needs every column, including
    /// ones that never change for the same download.
    private func refresh(_ positions: [Int], allColumns: Bool = false) {
        let visible = tableView.rows(in: tableView.visibleRect)
        for position in positions where NSLocationInRange(position, visible) {
            guard let task = store.row(ids[position]) else { continue }
            for (index, tableColumn) in tableView.tableColumns.enumerated() where !tableColumn.isHidden {
                guard let column = Column(rawValue: tableColumn.identifier.rawValue), allColumns || column != .added,
                      let view = tableView.view(atColumn: index, row: position, makeIfNecessary: false)
                else { continue }
                configure(view, column, task)
            }
        }
    }

    /// Recomputes the filtered, sorted view. Existing row views are updated
    /// in place: `reloadData` would rebuild every visible cell and its
    /// constraints, which costs far more than the sort itself.
    private func rebuild() {
        let selected = Set(selectedIDs())
        // Filter and sort indices; copying thousands of row structs per tick is the expensive part.
        let rows = store.rows
        var shown = rows.indices.filter { matches(rows[$0]) }
        if let descriptor = tableView.sortDescriptors.first, let key = descriptor.key, let column = Column(rawValue: key) {
            shown.sort { lhs, rhs in
                let order = compare(rows[lhs], rows[rhs], by: column)
                if order == .orderedSame { return rows[lhs].addedAt < rows[rhs].addedAt }
                return descriptor.ascending ? order == .orderedAscending : order == .orderedDescending
            }
        }
        let previousCount = ids.count
        ids = shown.map { rows[$0].id }
        positionByID = Dictionary(uniqueKeysWithValues: ids.enumerated().map { ($1, $0) })
        if ids.count != previousCount {
            tableView.noteNumberOfRowsChanged()
        }
        let visible = tableView.rows(in: tableView.visibleRect)
        refresh(Array(visible.location..<NSMaxRange(visible)), allColumns: true)
        tableView.selectRowIndexes(IndexSet(selected.compactMap { positionByID[$0] }), byExtendingSelection: false)

        if store.rows.isEmpty {
            emptyTitle.stringValue = "No Downloads"
            emptySubtitle.stringValue = "Press ⌘N or drop a link here to start one."
        } else {
            let (title, subtitle) = switch filter {
            case _ where !searchText.isEmpty: ("No Results", "No downloads match “\(searchText)”.")
            case .all: ("No Downloads", "Press ⌘N or drop a link here to start one.")
            case .unfinished: ("Nothing Unfinished", "Downloads still running, paused or failed appear here.")
            case .finished: ("Nothing Finished Yet", "Completed downloads appear here.")
            case .category(let name) where name == "Other": ("Nothing in Other", "Downloads that fit no other category appear here.")
            case .category(let name): ("Nothing in \(name)", "Downloads sorted into \(name) appear here.")
            }
            emptyTitle.stringValue = title
            emptySubtitle.stringValue = subtitle
        }
        emptyView.isHidden = !ids.isEmpty
    }

    private func matches(_ task: TaskRow) -> Bool {
        let inFilter = switch filter {
        case .all: true
        case .unfinished: task.status != .complete
        case .finished: task.status == .complete
        case .category(let name): task.category == name
        }
        guard inFilter else { return false }
        return searchText.isEmpty
            || task.name.localizedCaseInsensitiveContains(searchText)
            || task.url.localizedCaseInsensitiveContains(searchText)
    }

    private var sortColumn: Column? {
        tableView.sortDescriptors.first?.key.flatMap(Column.init(rawValue:))
    }

    private func sortValueChanged(_ old: TaskRow, _ new: TaskRow) -> Bool {
        guard let column = sortColumn else { return false }
        return compare(old, new, by: column) != .orderedSame
    }

    private func compare(_ lhs: TaskRow, _ rhs: TaskRow, by column: Column) -> ComparisonResult {
        func order<T: Comparable>(_ a: T, _ b: T) -> ComparisonResult {
            a < b ? .orderedAscending : (a > b ? .orderedDescending : .orderedSame)
        }
        switch column {
        case .name: return lhs.name.localizedStandardCompare(rhs.name)
        case .size: return order(lhs.totalBytes, rhs.totalBytes)
        case .progress: return order(lhs.progress, rhs.progress)
        case .speed: return order(lhs.speed, rhs.speed)
        case .eta: return order(lhs.etaSecs ?? .max, rhs.etaSecs ?? .max)
        case .status: return order(Format.status(lhs), Format.status(rhs))
        case .added: return order(lhs.addedAt, rhs.addedAt)
        case .main: return .orderedSame
        }
    }

    /// The current sort as (column key, ascending); nil means date added.
    var sort: (key: String, ascending: Bool)? {
        tableView.sortDescriptors.first.flatMap { descriptor in descriptor.key.map { ($0, descriptor.ascending) } }
    }

    /// Sorting for the Cards and List styles, which have no column headers.
    func setSort(key: String, ascending: Bool) {
        tableView.sortDescriptors = [NSSortDescriptor(key: key, ascending: ascending)]
    }

    // MARK: Table

    func numberOfRows(in tableView: NSTableView) -> Int {
        ids.count
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let tableColumn, let column = Column(rawValue: tableColumn.identifier.rawValue),
              let task = store.row(ids[row])
        else { return nil }
        let reuseIdentifier: NSUserInterfaceItemIdentifier = switch (column, style) {
        case (.main, .cards): CardCellView.reuseIdentifier
        case (.main, _): ListCellView.reuseIdentifier
        default: column.identifier
        }
        let view: NSView = tableView.makeView(withIdentifier: reuseIdentifier, owner: self) ?? {
            switch column {
            case .name: NameCellView(identifier: column.identifier)
            case .progress: ProgressCellView(identifier: column.identifier)
            case .status: StatusCellView(identifier: column.identifier)
            case .main where style == .cards: CardCellView()
            case .main: ListCellView()
            default: TextCellView(identifier: column.identifier, alignment: column.alignment)
            }
        }()
        configure(view, column, task)
        return view
    }

    func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? {
        style == .cards ? CardRowView() : nil
    }

    private func configure(_ view: NSView, _ column: Column, _ task: TaskRow) {
        switch view {
        case let cell as ProgressCellView:
            cell.show(task)
            return
        case let cell as StatusCellView:
            cell.show(task)
            return
        case let cell as CardCellView:
            cell.show(task)
            let id = task.id
            cell.onAction = { [weak self] action in self?.perform(action, id: id) }
            return
        case let cell as ListCellView:
            cell.show(task)
            let id = task.id
            cell.onAction = { [weak self] action in self?.perform(action, id: id) }
            return
        default:
            break
        }
        if column == .progress {
            (view as? ProgressCellView)?.show(task)
            return
        }
        guard let cell = view as? NSTableCellView, let field = cell.textField else { return }
        switch column {
        case .name:
            field.setIfChanged(task.name)
            let icon = FileIcons.icon(for: task.name)
            if cell.imageView?.image !== icon { cell.imageView?.image = icon }
        case .size: field.setIfChanged(Format.size(task))
        case .speed: field.setIfChanged(Format.speed(task.speed))
        case .eta: field.setIfChanged(Format.eta(task.etaSecs))
        case .added: field.setIfChanged(Format.date(task.addedAt))
        case .status, .progress, .main: break
        }
    }

    /// Buttons inside a row act on that row only.
    private func perform(_ action: RowAction, id: String) {
        guard let row = store.row(id) else { return }
        switch action {
        case .primary:
            switch row.shownStatus {
            case .complete: open([row])
            case .downloading, .queued: pause([row])
            case .paused, .failed: resume([row])
            }
        case .reveal: reveal([row])
        case .copyAddress: copyAddresses([row])
        case .remove: remove([row], deletingFiles: false)
        }
    }

    func tableView(_ tableView: NSTableView, sortDescriptorsDidChange oldDescriptors: [NSSortDescriptor]) {
        rebuild()
    }

    func tableView(_ tableView: NSTableView, validateDrop info: NSDraggingInfo, proposedRow row: Int,
                   proposedDropOperation dropOperation: NSTableView.DropOperation) -> NSDragOperation {
        guard !Self.links(from: info.draggingPasteboard).isEmpty else { return [] }
        tableView.setDropRow(-1, dropOperation: .on)
        return .copy
    }

    func tableView(_ tableView: NSTableView, acceptDrop info: NSDraggingInfo, row: Int,
                   dropOperation: NSTableView.DropOperation) -> Bool {
        let links = Self.links(from: info.draggingPasteboard)
        guard !links.isEmpty else { return false }
        NewDownloads.present(links.map { NewDownload(url: $0) }, core: core)
        return true
    }

    static func links(from pasteboard: NSPasteboard) -> [String] {
        let urls = (pasteboard.readObjects(forClasses: [NSURL.self]) as? [URL] ?? []).filter { !$0.isFileURL }.map(\.absoluteString)
        if !urls.isEmpty { return urls }
        return AddDownloadPrompt.links(in: pasteboard.string(forType: .string) ?? "")
    }

    // MARK: Selection and actions

    func selectedIDs() -> [String] {
        tableView.selectedRowIndexes.compactMap { $0 < ids.count ? ids[$0] : nil }
    }

    func selectedRows() -> [TaskRow] {
        selectedIDs().compactMap(store.row)
    }

    /// The clicked row, or the selection when the click is inside it.
    private func clickedRows() -> [TaskRow] {
        let clicked = tableView.clickedRow
        guard clicked >= 0, !tableView.selectedRowIndexes.contains(clicked) else { return selectedRows() }
        return store.row(ids[clicked]).map { [$0] } ?? []
    }

    func resume(_ rows: [TaskRow]) {
        let ids = rows.filter { $0.status == .paused || $0.status == .failed }.map(\.id)
        guard !ids.isEmpty else { return }
        core.resume(ids: ids)
        ProgressWindowController.show(ids: ids, core: core)
    }

    func pause(_ rows: [TaskRow]) {
        let ids = rows.filter(\.canPause).map(\.id)
        if !ids.isEmpty { core.pause(ids: ids) }
    }

    func remove(_ rows: [TaskRow], deletingFiles: Bool) {
        guard !rows.isEmpty else { return }
        guard deletingFiles, let window = view.window else {
            core.remove(ids: rows.map(\.id), deleteFiles: false)
            return
        }
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = rows.count == 1
            ? "Delete “\(rows[0].name)” from disk?"
            : "Delete the files of \(rows.count) downloads from disk?"
        alert.informativeText = rows.count == 1 ? "The file is deleted permanently." : "The files are deleted permanently."
        alert.addButton(withTitle: "Delete")
        alert.addButton(withTitle: "Cancel")
        alert.buttons[0].hasDestructiveAction = true
        alert.beginSheetModal(for: window) { [core] response in
            if response == .alertFirstButtonReturn {
                core.remove(ids: rows.map(\.id), deleteFiles: true)
            }
        }
    }

    func open(_ rows: [TaskRow]) {
        for row in rows where row.status == .complete {
            if let path = row.path { NSWorkspace.shared.open(URL(fileURLWithPath: path)) }
        }
    }

    func reveal(_ rows: [TaskRow]) {
        let urls = rows.map { row -> URL in
            if let path = row.path, FileManager.default.fileExists(atPath: path) { return URL(fileURLWithPath: path) }
            return URL(fileURLWithPath: row.dir)
        }
        NSWorkspace.shared.activateFileViewerSelecting(urls)
    }

    func copyAddresses(_ rows: [TaskRow]) {
        guard !rows.isEmpty else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(rows.map(\.url).joined(separator: "\n"), forType: .string)
    }

    @objc private func openClicked(_ sender: Any?) {
        guard tableView.clickedRow >= 0, let row = store.row(ids[tableView.clickedRow]) else { return }
        open([row])
    }

    private func handleKey(_ event: NSEvent) -> Bool {
        let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        switch event.keyCode {
        case 51, 117: // Delete, Forward Delete
            remove(selectedRows(), deletingFiles: modifiers.contains(.option))
        case 49: // Space toggles the selection between running and paused.
            let rows = selectedRows()
            if rows.contains(where: \.canPause) { pause(rows) } else { resume(rows) }
        case 36, 76: // Return, Enter
            open(selectedRows())
        default:
            return false
        }
        return true
    }

    // MARK: Menus

    func menuNeedsUpdate(_ menu: NSMenu) {
        guard menu === tableView.menu else { return }
        menu.removeAllItems()
        let rows = clickedRows()
        guard !rows.isEmpty else { return }
        func add(_ title: String, _ action: Selector, enabled: Bool = true) {
            let item = menu.addItem(withTitle: title, action: enabled ? action : nil, keyEquivalent: "")
            item.target = self
        }
        add("Open", #selector(contextOpen), enabled: rows.contains { $0.status == .complete })
        add("Show in Finder", #selector(contextReveal))
        menu.addItem(.separator())
        add("Resume", #selector(contextResume), enabled: rows.contains { $0.status == .paused || $0.status == .failed })
        add("Pause", #selector(contextPause), enabled: rows.contains(where: \.canPause))
        menu.addItem(.separator())
        add("Copy Address", #selector(contextCopy))
        add("Refresh Download Address…", #selector(contextRefreshAddress),
            enabled: rows.count == 1 && rows[0].status != .complete)
        menu.addItem(.separator())
        add("Remove", #selector(contextRemove))
        add("Remove and Delete Files…", #selector(contextDelete))
    }

    @objc private func contextOpen() { open(clickedRows()) }
    @objc private func contextReveal() { reveal(clickedRows()) }
    @objc private func contextResume() { resume(clickedRows()) }
    @objc private func contextPause() { pause(clickedRows()) }
    @objc private func contextCopy() { copyAddresses(clickedRows()) }
    @objc private func contextRefreshAddress() {
        guard let row = clickedRows().first else { return }
        AddressRefresh.start(id: row.id, core: core)
    }
    @objc private func contextRemove() { remove(clickedRows(), deletingFiles: false) }
    @objc private func contextDelete() { remove(clickedRows(), deletingFiles: true) }

    /// Right-click on the header shows or hides columns.
    private func columnMenu() -> NSMenu {
        let menu = NSMenu()
        for column in Column.allCases where column != .name && column != .main {
            let item = menu.addItem(withTitle: column.title, action: #selector(toggleColumn(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = column.rawValue
        }
        menu.delegate = self
        return menu
    }

    func menuWillOpen(_ menu: NSMenu) {
        guard menu === tableView.headerView?.menu else { return }
        for item in menu.items {
            let column = (item.representedObject as? String).flatMap { tableView.tableColumn(withIdentifier: .init($0)) }
            item.state = column?.isHidden == false ? .on : .off
        }
    }

    @objc private func toggleColumn(_ sender: NSMenuItem) {
        guard let id = sender.representedObject as? String, let column = tableView.tableColumn(withIdentifier: .init(id)) else { return }
        column.isHidden.toggle()
        if !column.isHidden { rebuild() }
    }
}

/// Forwards key presses the list handles itself (Delete, Space, Return).
final class DownloadTableView: NSTableView {
    var onKey: ((NSEvent) -> Bool)?

    override func keyDown(with event: NSEvent) {
        if onKey?(event) == true { return }
        super.keyDown(with: event)
    }
}
