import AppKit
import OpenDMCore

/// Icon and file name.
final class NameCellView: NSTableCellView {
    init(identifier: NSUserInterfaceItemIdentifier) {
        super.init(frame: .zero)
        self.identifier = identifier
        let icon = NSImageView()
        let label = NSTextField(labelWithString: "")
        label.lineBreakMode = .byTruncatingMiddle
        for view in [icon, label] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        imageView = icon
        textField = label
        NSLayoutConstraint.activate([
            icon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 2),
            icon.centerYAnchor.constraint(equalTo: centerYAnchor),
            icon.widthAnchor.constraint(equalToConstant: 16),
            icon.heightAnchor.constraint(equalToConstant: 16),
            label.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: 6),
            label.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -2),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError("not used") }
}

/// One line of text; numbers use tabular digits so columns do not jitter.
final class TextCellView: NSTableCellView {
    init(identifier: NSUserInterfaceItemIdentifier, alignment: NSTextAlignment) {
        super.init(frame: .zero)
        self.identifier = identifier
        let label = NSTextField(labelWithString: "")
        label.alignment = alignment
        label.lineBreakMode = .byTruncatingTail
        label.font = .monospacedDigitSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)
        label.translatesAutoresizingMaskIntoConstraints = false
        addSubview(label)
        textField = label
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 2),
            label.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -2),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError("not used") }
}

/// A thin bar with the percentage beside it.
final class ProgressCellView: NSTableCellView {
    private let bar = ProgressBar()
    private let label = NSTextField(labelWithString: "")

    init(identifier: NSUserInterfaceItemIdentifier) {
        super.init(frame: .zero)
        self.identifier = identifier
        label.alignment = .right
        label.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        label.textColor = .secondaryLabelColor
        for view in [bar, label] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        NSLayoutConstraint.activate([
            bar.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 2),
            bar.centerYAnchor.constraint(equalTo: centerYAnchor),
            bar.heightAnchor.constraint(equalToConstant: 6),
            label.leadingAnchor.constraint(equalTo: bar.trailingAnchor, constant: 6),
            label.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -2),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
            label.widthAnchor.constraint(equalToConstant: 48),
        ])
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    func show(_ task: TaskRow) {
        // Only show progress that means something: not for finished rows or
        // for ones that have not started.
        let shows = task.status != .complete && (task.progress > 0 || task.status == .downloading)
        if bar.isHidden == shows { bar.isHidden = !shows }
        bar.update(progress: max(task.progress, 0), color: StatusLook.of(task).color)
        label.setIfChanged(shows ? Format.percent(task) : "")
    }
}

/// Colored status symbol and text.
final class StatusCellView: NSTableCellView {
    private let symbol = NSImageView()
    private var symbolName = ""

    init(identifier: NSUserInterfaceItemIdentifier) {
        super.init(frame: .zero)
        self.identifier = identifier
        let label = NSTextField(labelWithString: "")
        label.lineBreakMode = .byTruncatingTail
        label.textColor = .secondaryLabelColor
        label.setContentHuggingPriority(.defaultLow, for: .horizontal)
        for view in [symbol, label] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        textField = label
        NSLayoutConstraint.activate([
            symbol.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 2),
            symbol.centerYAnchor.constraint(equalTo: centerYAnchor),
            symbol.widthAnchor.constraint(equalToConstant: 14),
            label.leadingAnchor.constraint(equalTo: symbol.trailingAnchor, constant: 5),
            label.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -2),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    func show(_ task: TaskRow) {
        let look = StatusLook.of(task)
        if look.symbol != symbolName {
            symbolName = look.symbol
            symbol.image = NSImage(systemSymbolName: look.symbol, accessibilityDescription: nil)?
                .withSymbolConfiguration(.init(pointSize: 11, weight: .semibold))
        }
        symbol.contentTintColor = look.color
        textField?.setIfChanged(look.title)
        toolTip = task.error
    }
}

final class ProgressBar: NSView {
    private var progress: Double = -1
    private var color: NSColor = .controlAccentColor

    func update(progress: Double, color: NSColor) {
        guard progress != self.progress || color != self.color else { return }
        self.progress = progress
        self.color = color
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        let radius = bounds.height / 2
        NSColor.quaternaryLabelColor.setFill()
        NSBezierPath(roundedRect: bounds, xRadius: radius, yRadius: radius).fill()
        guard progress > 0 else { return }
        var filled = bounds
        filled.size.width = max(bounds.height, bounds.width * CGFloat(min(progress, 1)))
        color.setFill()
        NSBezierPath(roundedRect: filled, xRadius: radius, yRadius: radius).fill()
    }
}

extension NSTextField {
    /// Skips redundant layout and drawing when a value repeats every tick.
    func setIfChanged(_ value: String) {
        if stringValue != value { stringValue = value }
    }
}
