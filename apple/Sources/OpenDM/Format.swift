import AppKit
import OpenDMCore
import UniformTypeIdentifiers

@MainActor
enum Format {
    private static let byteFormatter: ByteCountFormatter = {
        let formatter = ByteCountFormatter()
        formatter.countStyle = .file
        formatter.allowsNonnumericFormatting = false
        return formatter
    }()

    private static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .short
        formatter.timeStyle = .short
        formatter.doesRelativeDateFormatting = true
        return formatter
    }()

    static func bytes(_ count: UInt64) -> String {
        byteFormatter.string(fromByteCount: Int64(clamping: count))
    }

    static func size(_ task: TaskRow) -> String {
        task.totalBytes > 0 ? bytes(task.totalBytes) : "–"
    }

    static func speed(_ bytesPerSecond: UInt64) -> String {
        bytesPerSecond > 0 ? bytes(bytesPerSecond) + "/s" : ""
    }

    static func percent(_ task: TaskRow) -> String {
        task.progress < 0 ? "" : "\(Int((task.progress * 100).rounded(.down)))%"
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
        case .complete: return "Complete"
        case .failed: return task.error ?? "Failed"
        }
    }

    static func date(_ unixMilliseconds: Int64) -> String {
        dateFormatter.string(from: Date(timeIntervalSince1970: TimeInterval(unixMilliseconds) / 1000))
    }
}

@MainActor
enum FileIcons {
    private static var cache: [String: NSImage] = [:]

    /// The system icon for a file name's type, cached per extension.
    static func icon(for name: String) -> NSImage {
        let pathExtension = (name as NSString).pathExtension.lowercased()
        if let cached = cache[pathExtension] { return cached }
        let type = UTType(filenameExtension: pathExtension) ?? .data
        let icon = NSWorkspace.shared.icon(for: type)
        icon.size = NSSize(width: 16, height: 16)
        cache[pathExtension] = icon
        return icon
    }
}
