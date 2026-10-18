import AppKit
import OpenDMCore

/// Source list of views (All, Unfinished, Finished) and file categories,
/// each with its count.
final class SidebarViewController: NSViewController, NSTableViewDataSource, NSTableViewDelegate, DownloadStoreObserver {
    private enum Item {
        case header(String)
        case filter(ListFilter, title: String, symbol: String)
    }

    var onSelect: ((ListFilter) -> Void)?
    private let store: DownloadStore
    private let tableView = NSTableView()
    private let items: [Item]
    private var counts: [ListFilter: Int] = [:]

    init(store: DownloadStore, categories: [String]) {
        self.store = store
        var items: [Item] = [
            .filter(.all, title: "All Downloads", symbol: "tray.full"),
            .filter(.unfinished, title: "Unfinished", symbol: "arrow.down.circle"),
            .filter(.finished, title: "Finished", symbol: "checkmark.circle"),
            .header("Categories"),
        ]
        for name in categories {
            items.append(.filter(.category(name), title: name, symbol: Self.symbol(for: name)))
        }
        self.items = items
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    private static func symbol(for category: String) -> String {
        switch category {
        case "Compressed": "doc.zipper"
        case "Documents": "doc.text"
        case "Music": "music.note"
        case "Programs": "shippingbox"
        case "Video": "film"
        default: "folder"
        }
    }

    override func loadView() {
        let column = NSTableColumn(identifier: .init("item"))
        tableView.addTableColumn(column)
        tableView.headerView = nil
        tableView.style = .sourceList
        tableView.floatsGroupRows = false
        tableView.dataSource = self
        tableView.delegate = self
        let scrollView = NSScrollView()
        scrollView.documentView = tableView
        scrollView.drawsBackground = false
        view = scrollView
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        store.addObserver(self)
        recount()
        tableView.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
    }

    func store(_ store: DownloadStore, didChange changes: [RowChange]) {
        let affectsCounts = changes.contains { change in
            guard let old = change.old else { return true }
            return (old.status == .complete) != (change.new.status == .complete) || old.category != change.new.category
        }
        if affectsCounts { recount() }
    }

    func store(_ store: DownloadStore, didRemove ids: Set<String>) {
        recount()
    }

    private func recount() {
        var counts: [ListFilter: Int] = [:]
        for row in store.rows {
            counts[.all, default: 0] += 1
            counts[row.status == .complete ? .finished : .unfinished, default: 0] += 1
            counts[.category(row.category), default: 0] += 1
        }
        guard counts != self.counts else { return }
        self.counts = counts
        let selection = tableView.selectedRowIndexes
        tableView.reloadData()
        tableView.selectRowIndexes(selection, byExtendingSelection: false)
    }

    func numberOfRows(in tableView: NSTableView) -> Int {
        items.count
    }

    func tableView(_ tableView: NSTableView, isGroupRow row: Int) -> Bool {
        if case .header = items[row] { return true }
        return false
    }

    func tableView(_ tableView: NSTableView, shouldSelectRow row: Int) -> Bool {
        !self.tableView(tableView, isGroupRow: row)
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        switch items[row] {
        case .header(let title):
            let cell = NSTableCellView()
            let label = NSTextField(labelWithString: title)
            label.translatesAutoresizingMaskIntoConstraints = false
            cell.addSubview(label)
            cell.textField = label
            NSLayoutConstraint.activate([
                label.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 2),
                label.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
            ])
            return cell
        case .filter(let filter, let title, let symbol):
            let cell = tableView.makeView(withIdentifier: .init("filter"), owner: self) as? SidebarCellView
                ?? SidebarCellView()
            cell.show(title: title, symbol: symbol, count: counts[filter] ?? 0)
            return cell
        }
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        guard tableView.selectedRow >= 0, case .filter(let filter, _, _) = items[tableView.selectedRow] else { return }
        onSelect?(filter)
    }
}

private final class SidebarCellView: NSTableCellView {
    private let badge = CountBadge()

    init() {
        super.init(frame: .zero)
        identifier = .init("filter")
        let icon = NSImageView()
        let label = NSTextField(labelWithString: "")
        label.lineBreakMode = .byTruncatingTail
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        badge.setContentCompressionResistancePriority(.required, for: .horizontal)
        for view in [icon, label, badge] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        imageView = icon
        textField = label
        NSLayoutConstraint.activate([
            icon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 2),
            icon.centerYAnchor.constraint(equalTo: centerYAnchor),
            icon.widthAnchor.constraint(equalToConstant: 18),
            label.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: 6),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
            badge.leadingAnchor.constraint(greaterThanOrEqualTo: label.trailingAnchor, constant: 6),
            badge.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -4),
            badge.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    func show(title: String, symbol: String, count: Int) {
        textField?.stringValue = title
        imageView?.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)
        badge.count = count
    }
}

/// A small pill with a count, hidden at zero.
private final class CountBadge: NSView {
    var count = 0 {
        didSet {
            guard count != oldValue else { return }
            isHidden = count == 0
            invalidateIntrinsicContentSize()
            needsDisplay = true
        }
    }

    private var text: NSAttributedString {
        NSAttributedString(
            string: NumberFormatter.localizedString(from: NSNumber(value: count), number: .decimal),
            attributes: [
                .font: NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .medium),
                .foregroundColor: NSColor.secondaryLabelColor,
            ]
        )
    }

    override init(frame: NSRect) {
        super.init(frame: frame)
        isHidden = true
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    override var intrinsicContentSize: NSSize {
        NSSize(width: max(ceil(text.size().width) + 12, 20), height: 17)
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.quaternaryLabelColor.setFill()
        NSBezierPath(roundedRect: bounds, xRadius: bounds.height / 2, yRadius: bounds.height / 2).fill()
        let size = text.size()
        text.draw(at: NSPoint(x: (bounds.width - size.width) / 2, y: (bounds.height - size.height) / 2))
    }
}
