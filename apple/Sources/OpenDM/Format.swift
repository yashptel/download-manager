import AppKit
import OpenDMCore
import UniformTypeIdentifiers

@MainActor
enum Format {
    private static let units = ["KB", "MB", "GB", "TB"]

    private static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .short
        formatter.timeStyle = .short
        formatter.doesRelativeDateFormatting = true
        return formatter
    }()

    /// "12.34 MB": two decimals, in the 1000-byte units Finder uses.
    static func bytes(_ count: UInt64) -> String {
        guard count >= 1000 else { return "\(count) bytes" }
        var value = Double(count) / 1000
        var unit = 0
        while value >= 1000 && unit < units.count - 1 {
            value /= 1000
            unit += 1
        }
        return value.formatted(.number.precision(.fractionLength(2))) + " " + units[unit]
    }

    static func size(_ task: TaskRow) -> String {
        task.totalBytes > 0 ? bytes(task.totalBytes) : task.doneBytes > 0 ? bytes(task.doneBytes) : ""
    }

    static func speed(_ bytesPerSecond: UInt64) -> String {
        bytesPerSecond > 0 ? bytes(bytesPerSecond) + "/s" : ""
    }

    /// "44.53%"; rounded down, so 100.00% means done.
    static func percent(_ task: TaskRow) -> String {
        guard task.progress >= 0 else { return "" }
        return ((task.progress * 10_000).rounded(.down) / 100).formatted(.number.precision(.fractionLength(2))) + "%"
    }

    static func eta(_ seconds: UInt64?) -> String {
        guard let seconds else { return "" }
        switch seconds {
        case ..<60: return "\(seconds)s"
        case ..<3600: return "\(seconds / 60)m \(seconds % 60)s"
        case ..<86_400: return "\(seconds / 3600)h \(seconds % 3600 / 60)m"
        default: return "\(seconds / 86_400)d \(seconds % 86_400 / 3600)h"
        }
    }

    static func status(_ task: TaskRow) -> String {
        switch task.status {
        case .downloading: return "Downloading"
        case .queued: return "Queued"
        case .paused: return "Paused"
        case .complete: return "Completed"
        case .failed: return [task.error ?? "Failed", retrying(task)].compactMap { $0 }.joined(separator: " · ")
        }
    }

    /// "Retrying at 10:52", for a failed download the app tries again on its own.
    static func retrying(_ task: TaskRow) -> String? {
        guard let at = task.retryAt else { return nil }
        return "Retrying at " + Date(timeIntervalSince1970: TimeInterval(at) / 1000).formatted(date: .omitted, time: .shortened)
    }

    static func date(_ unixMilliseconds: Int64) -> String {
        dateFormatter.string(from: Date(timeIntervalSince1970: TimeInterval(unixMilliseconds) / 1000))
    }

    /// "45 sec", "4 min 47 sec", "2 hr 5 min", "3 days 4 hr": the progress
    /// window's time left, in the units the list uses.
    static func timeLeft(_ seconds: UInt64?) -> String {
        guard let seconds else { return "" }
        switch seconds {
        case ..<60: return "\(seconds) sec"
        case ..<3600: return "\(seconds / 60) min \(seconds % 60) sec"
        case ..<86_400: return "\(seconds / 3600) hr \(seconds % 3600 / 60) min"
        default:
            let days = seconds / 86_400
            return "\(days) \(days == 1 ? "day" : "days") \(seconds % 86_400 / 3600) hr"
        }
    }

    /// "3 min left", "45 sec left", "2 hr 5 min left".
    static func remaining(_ seconds: UInt64?) -> String? {
        guard let seconds else { return nil }
        switch seconds {
        case ..<60: return "\(seconds) sec left"
        case ..<3600: return "\(seconds / 60) min left"
        default: return "\(seconds / 3600) hr \(seconds % 3600 / 60) min left"
        }
    }

    /// "88.1 MB of 209.7 MB", or what is known so far.
    static func amount(_ task: TaskRow) -> String? {
        switch (task.doneBytes, task.totalBytes) {
        case (_, 0) where task.doneBytes > 0: return bytes(task.doneBytes)
        case (_, 0): return nil
        case (0, let total): return bytes(total)
        case (let done, let total) where done >= total: return bytes(total)
        case (let done, let total): return "\(bytes(done)) of \(bytes(total))"
        }
    }

    /// What waits when nothing downloads: "2 paused", "1 queued · 3 paused".
    static func idle(_ rows: [TaskRow]) -> String {
        let queued = rows.count { $0.status == .queued }
        let paused = rows.count { $0.status == .paused }
        return [queued > 0 ? "\(queued) queued" : nil, paused > 0 ? "\(paused) paused" : nil]
            .compactMap { $0 }
            .joined(separator: " · ")
    }

    /// One line describing where a download stands, for row layouts.
    static func detail(_ task: TaskRow) -> String {
        let parts: [String?] = switch task.status {
        case .downloading: [amount(task), task.speed > 0 ? speed(task.speed) : nil, remaining(task.etaSecs)]
        case .queued: ["Waiting", amount(task)]
        case .paused: ["Paused", amount(task)]
        case .complete: [task.totalBytes > 0 ? bytes(task.totalBytes) : "Completed"]
        case .failed: [task.error ?? "Failed", retrying(task)]
        }
        return parts.compactMap { $0 }.joined(separator: " · ")
    }
}

/// How a status looks: symbol, tint and short title.
struct StatusLook {
    let symbol: String
    let color: NSColor
    let title: String

    @MainActor
    static func of(_ task: TaskRow) -> StatusLook {
        // A download waiting to try again looks queued, not failed.
        if task.status == .failed, let retrying = Format.retrying(task) {
            return StatusLook(symbol: "clock.fill", color: .tertiaryLabelColor, title: retrying)
        }
        return switch task.status {
        case .downloading: StatusLook(symbol: "arrow.down.circle.fill", color: .controlAccentColor, title: "Downloading")
        case .queued: StatusLook(symbol: "clock.fill", color: .tertiaryLabelColor, title: "Queued")
        case .paused: StatusLook(symbol: "pause.circle.fill", color: .systemOrange, title: "Paused")
        case .complete: StatusLook(symbol: "checkmark.circle.fill", color: .systemGreen, title: "Completed")
        case .failed: StatusLook(symbol: "exclamationmark.triangle.fill", color: .systemRed, title: "Failed")
        }
    }
}

@MainActor
enum FileIcons {
    private static var cache: [String: NSImage] = [:]

    /// The system icon for a file name's type, cached per extension and size.
    static func icon(for name: String, size: CGFloat = 16) -> NSImage {
        let pathExtension = (name as NSString).pathExtension.lowercased()
        let key = "\(pathExtension)@\(Int(size))"
        if let cached = cache[key] { return cached }
        let type = UTType(filenameExtension: pathExtension) ?? .data
        let icon = NSWorkspace.shared.icon(for: type).copy() as! NSImage
        icon.size = NSSize(width: size, height: size)
        cache[key] = icon
        return icon
    }
}

/// A borderless SF Symbol button used for row actions.
@MainActor
func iconButton(_ symbol: String, _ label: String, pointSize: CGFloat = 14) -> NSButton {
    let image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)?
        .withSymbolConfiguration(.init(pointSize: pointSize, weight: .regular))
    let button = NSButton(image: image ?? NSImage(), target: nil, action: nil)
    button.isBordered = false
    button.toolTip = label
    button.contentTintColor = .secondaryLabelColor
    button.setAccessibilityLabel(label)
    return button
}

extension Error {
    /// The core's message on its own: UniFFI describes its errors as the
    /// Swift case, such as Failed(message: "...").
    var message: String {
        if let error = self as? CoreError, case .Failed(let message) = error { return message }
        return localizedDescription
    }
}
