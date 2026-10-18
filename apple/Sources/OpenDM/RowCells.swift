import AppKit
import OpenDMCore

// MARK: Cards

/// Draws each row as a rounded card; selection is an accent outline, so the
/// card's text keeps its normal colors.
final class CardRowView: NSTableRowView {
    private static let cardFill = NSColor(name: nil) { appearance in
        appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            ? NSColor.white.withAlphaComponent(0.06)
            : NSColor.white
    }

    override func drawBackground(in dirtyRect: NSRect) {
        let card = bounds.insetBy(dx: Self.margin, dy: 4)
        let path = NSBezierPath(roundedRect: card, xRadius: 10, yRadius: 10)
        Self.cardFill.setFill()
        path.fill()
        if isSelected {
            NSColor.controlAccentColor.withAlphaComponent(0.08).setFill()
            path.fill()
            NSColor.controlAccentColor.setStroke()
            path.lineWidth = 2
        } else {
            NSColor.separatorColor.setStroke()
            path.lineWidth = 1
        }
        path.stroke()
    }

    override func drawSelection(in dirtyRect: NSRect) {}

    override var interiorBackgroundStyle: NSView.BackgroundStyle { .normal }

    /// Space between the card and the edges of the list.
    static let margin: CGFloat = 10
}

/// Rayburst-like card: name, colored status line, full-width bar, size line,
/// and an always-visible action pill.
final class CardCellView: NSTableCellView {
    static let reuseIdentifier = NSUserInterfaceItemIdentifier("card")
    var onAction: ((RowAction) -> Void)?

    private let icon = NSImageView()
    private let name = NSTextField(labelWithString: "")
    private let statusIcon = NSImageView()
    private let statusText = NSTextField(labelWithString: "")
    private let bar = ProgressBar()
    private let amountText = NSTextField(labelWithString: "")
    private let primaryButton = iconButton("pause", "Pause", pointSize: 13)
    private let pill = PillView()
    private var primarySymbol = ""
    private var statusSymbol = ""

    init() {
        super.init(frame: .zero)
        identifier = Self.reuseIdentifier
        name.font = .systemFont(ofSize: 13, weight: .semibold)
        name.lineBreakMode = .byTruncatingMiddle
        statusText.font = .systemFont(ofSize: 12)
        statusText.lineBreakMode = .byTruncatingTail
        amountText.font = .monospacedDigitSystemFont(ofSize: 11, weight: .regular)
        amountText.textColor = .secondaryLabelColor
        name.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        statusText.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

        let buttons = [
            primaryButton,
            iconButton("folder", "Show in Finder", pointSize: 13),
            iconButton("link", "Copy Address", pointSize: 13),
            iconButton("trash", "Remove", pointSize: 13),
        ]
        let actions: [RowAction] = [.primary, .reveal, .copyAddress, .remove]
        for (button, action) in zip(buttons, actions) {
            button.target = self
            button.action = #selector(buttonPressed(_:))
            button.tag = actions.firstIndex(of: action)!
        }
        let stack = NSStackView(views: buttons)
        stack.spacing = 14
        stack.edgeInsets = NSEdgeInsets(top: 5, left: 12, bottom: 5, right: 12)
        stack.translatesAutoresizingMaskIntoConstraints = false
        // Equal slots, so the pill is the same width whatever the first symbol is.
        for button in buttons {
            button.widthAnchor.constraint(equalToConstant: 18).isActive = true
        }
        pill.addSubview(stack)

        for view in [icon, name, statusIcon, statusText, bar, amountText, pill] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        imageView = icon
        textField = name
        NSLayoutConstraint.activate([
            stack.topAnchor.constraint(equalTo: pill.topAnchor),
            stack.bottomAnchor.constraint(equalTo: pill.bottomAnchor),
            stack.leadingAnchor.constraint(equalTo: pill.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: pill.trailingAnchor),

            icon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: CardRowView.margin + 14),
            icon.topAnchor.constraint(equalTo: topAnchor, constant: 18),
            icon.widthAnchor.constraint(equalToConstant: 36),
            icon.heightAnchor.constraint(equalToConstant: 36),

            pill.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -(CardRowView.margin + 14)),
            pill.topAnchor.constraint(equalTo: topAnchor, constant: 13),

            name.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: 12),
            name.topAnchor.constraint(equalTo: topAnchor, constant: 17),
            name.trailingAnchor.constraint(lessThanOrEqualTo: pill.leadingAnchor, constant: -12),

            statusIcon.leadingAnchor.constraint(equalTo: name.leadingAnchor),
            statusIcon.centerYAnchor.constraint(equalTo: statusText.centerYAnchor),
            statusText.leadingAnchor.constraint(equalTo: statusIcon.trailingAnchor, constant: 5),
            statusText.topAnchor.constraint(equalTo: name.bottomAnchor, constant: 4),
            statusText.trailingAnchor.constraint(lessThanOrEqualTo: pill.leadingAnchor, constant: -12),

            bar.leadingAnchor.constraint(equalTo: name.leadingAnchor),
            bar.trailingAnchor.constraint(equalTo: pill.trailingAnchor),
            bar.topAnchor.constraint(equalTo: statusText.bottomAnchor, constant: 9),
            bar.heightAnchor.constraint(equalToConstant: 5),

            amountText.leadingAnchor.constraint(equalTo: name.leadingAnchor),
            amountText.topAnchor.constraint(equalTo: bar.bottomAnchor, constant: 7),
        ])
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    func show(_ task: TaskRow) {
        let look = StatusLook.of(task)
        name.setIfChanged(task.name)
        let fileIcon = FileIcons.icon(for: task.name, size: 36)
        if icon.image !== fileIcon { icon.image = fileIcon }
        if statusSymbol != look.symbol {
            statusSymbol = look.symbol
            statusIcon.image = NSImage(systemSymbolName: look.symbol, accessibilityDescription: nil)?
                .withSymbolConfiguration(.init(pointSize: 11, weight: .semibold))
        }
        statusIcon.contentTintColor = look.color
        statusIcon.setAccessibilityLabel(look.title)
        statusText.textColor = task.status == .failed ? .systemRed : look.color == .tertiaryLabelColor ? .secondaryLabelColor : look.color

        var status = [task.status == .failed ? task.error ?? look.title : look.title]
        if task.status == .downloading {
            if task.speed > 0 { status.append(Format.speed(task.speed)) }
            if let left = Format.remaining(task.etaSecs) { status.append(left) }
        }
        statusText.setIfChanged(status.joined(separator: " · "))
        statusText.toolTip = task.status == .failed ? task.error : nil
        bar.update(progress: max(task.progress, 0), color: look.color)

        let percent = task.progress >= 0 ? "\(Int((task.progress * 100).rounded(.down)))%" : nil
        let size: String? = switch (task.doneBytes, task.totalBytes) {
        case (_, 0) where task.doneBytes > 0: Format.bytes(task.doneBytes)
        case (_, 0): nil
        case (let done, let total): "\(Format.bytes(min(done, total))) / \(Format.bytes(total))"
        }
        let amount: String = switch task.status {
        case .complete: Format.size(task)
        case _ where task.doneBytes == 0 && task.totalBytes == 0: "Not started"
        default: [percent, size].compactMap { $0 }.joined(separator: " · ")
        }
        amountText.setIfChanged(amount)

        let (symbol, label): (String, String) = switch task.status {
        case .downloading, .queued: ("pause", "Pause")
        case .paused: ("play", "Resume")
        case .failed: ("arrow.clockwise", "Retry")
        case .complete: ("arrow.up.forward.app", "Open")
        }
        if symbol != primarySymbol {
            primarySymbol = symbol
            primaryButton.image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)?
                .withSymbolConfiguration(.init(pointSize: 13, weight: .regular))
            primaryButton.toolTip = label
            primaryButton.setAccessibilityLabel(label)
        }
    }

    @objc private func buttonPressed(_ sender: NSButton) {
        let actions: [RowAction] = [.primary, .reveal, .copyAddress, .remove]
        onAction?(actions[sender.tag])
    }
}

/// Rounded outline around the card's buttons.
final class PillView: NSView {
    override func draw(_ dirtyRect: NSRect) {
        let path = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: bounds.height / 2, yRadius: bounds.height / 2)
        NSColor.separatorColor.setStroke()
        path.stroke()
    }
}

// MARK: List

/// Safari/Transmission-like row: large file icon, name, thin bar while
/// unfinished, one detail line; circular buttons on the right, in fixed slots
/// so they line up from row to row.
final class ListCellView: NSTableCellView {
    static let reuseIdentifier = NSUserInterfaceItemIdentifier("list")
    var onAction: ((RowAction) -> Void)?

    private let icon = NSImageView()
    private let name = NSTextField(labelWithString: "")
    private let bar = ProgressBar()
    private let detail = NSTextField(labelWithString: "")
    private let primaryButton = iconButton("pause.circle.fill", "Pause", pointSize: 17)
    private let revealButton = iconButton("magnifyingglass.circle.fill", "Show in Finder", pointSize: 17)
    private var status: TaskStatus = .queued
    private var primarySymbol = ""
    private var barColor: NSColor = .controlAccentColor
    private var progress: Double = 0

    init() {
        super.init(frame: .zero)
        identifier = Self.reuseIdentifier
        name.font = .systemFont(ofSize: 13)
        name.lineBreakMode = .byTruncatingMiddle
        detail.font = .monospacedDigitSystemFont(ofSize: 11, weight: .regular)
        detail.lineBreakMode = .byTruncatingTail
        for label in [name, detail] {
            label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        }
        primaryButton.target = self
        primaryButton.action = #selector(primaryPressed)
        revealButton.target = self
        revealButton.action = #selector(revealPressed)

        let text = NSStackView(views: [name, bar, detail])
        text.orientation = .vertical
        text.alignment = .leading
        text.spacing = 3
        for view in [icon, text, primaryButton, revealButton] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        imageView = icon
        textField = name
        // Fixed slots for the two buttons at the trailing edge; the text takes the rest.
        NSLayoutConstraint.activate([
            icon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 6),
            icon.centerYAnchor.constraint(equalTo: centerYAnchor),
            icon.widthAnchor.constraint(equalToConstant: 32),
            icon.heightAnchor.constraint(equalToConstant: 32),
            text.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: 10),
            text.centerYAnchor.constraint(equalTo: centerYAnchor),
            text.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -72),
            bar.heightAnchor.constraint(equalToConstant: 4),
            bar.widthAnchor.constraint(equalTo: text.widthAnchor),
            revealButton.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            revealButton.centerYAnchor.constraint(equalTo: centerYAnchor),
            revealButton.widthAnchor.constraint(equalToConstant: 22),
            primaryButton.trailingAnchor.constraint(equalTo: revealButton.leadingAnchor, constant: -6),
            primaryButton.centerYAnchor.constraint(equalTo: centerYAnchor),
            primaryButton.widthAnchor.constraint(equalToConstant: 22),
        ])
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    func show(_ task: TaskRow) {
        status = task.status
        name.setIfChanged(task.name)
        let fileIcon = FileIcons.icon(for: task.name, size: 32)
        if icon.image !== fileIcon { icon.image = fileIcon }
        detail.setIfChanged(Format.detail(task))
        detail.toolTip = task.status == .failed ? task.error : nil
        progress = max(task.progress, 0)
        barColor = StatusLook.of(task).color
        let showsBar = task.status != .complete && (task.progress > 0 || task.status == .downloading)
        if bar.isHidden == showsBar { bar.isHidden = !showsBar }

        let (symbol, label): (String, String) = switch task.status {
        case .downloading, .queued: ("pause.circle.fill", "Pause")
        case .paused: ("play.circle.fill", "Resume")
        case .failed: ("arrow.clockwise.circle.fill", "Retry")
        case .complete: ("", "")
        }
        if symbol != primarySymbol, !symbol.isEmpty {
            primaryButton.image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)?
                .withSymbolConfiguration(.init(pointSize: 17, weight: .regular))
            primaryButton.toolTip = label
            primaryButton.setAccessibilityLabel(label)
        }
        primarySymbol = symbol
        primaryButton.isHidden = task.status == .complete
        updateColors()
    }

    override var backgroundStyle: NSView.BackgroundStyle {
        didSet { updateColors() }
    }

    private func updateColors() {
        let emphasized = backgroundStyle == .emphasized
        detail.textColor = emphasized
            ? .alternateSelectedControlTextColor.withAlphaComponent(0.8)
            : status == .failed ? .systemRed : .secondaryLabelColor
        bar.update(progress: progress, color: emphasized ? .alternateSelectedControlTextColor : barColor)
        for button in [primaryButton, revealButton] {
            button.contentTintColor = emphasized ? .alternateSelectedControlTextColor : .tertiaryLabelColor
        }
    }

    @objc private func primaryPressed() { onAction?(.primary) }
    @objc private func revealPressed() { onAction?(.reveal) }
}
