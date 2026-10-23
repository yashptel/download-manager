import Foundation
import OpenDMCore

/// What the menu bar and Dock icons say about the downloads. A fraction is
/// the share of the known bytes that are done, or nil when no download in
/// that state knows its size yet.
enum GlyphState: Equatable {
    case idle
    case downloading(fraction: Double?)
    case paused(fraction: Double?)
    case completed

    static func from(_ rows: [TaskRow]) -> GlyphState {
        let active = rows.filter { $0.status == .downloading }
        if !active.isEmpty { return .downloading(fraction: fraction(of: active)) }
        let paused = rows.filter { $0.status == .paused }
        if !paused.isEmpty { return .paused(fraction: fraction(of: paused)) }
        return .idle
    }

    private static func fraction(of rows: [TaskRow]) -> Double? {
        let known = rows.filter { $0.totalBytes > 0 }
        let total = known.reduce(0.0) { $0 + Double($1.totalBytes) }
        guard total > 0 else { return nil }
        let done = known.reduce(0.0) { $0 + Double($1.doneBytes) }
        return min(max(done / total, 0), 1)
    }

    var isDownloading: Bool {
        if case .downloading = self { true } else { false }
    }

    var frame: GlyphFrame {
        switch self {
        case .idle: GlyphFrame(progress: nil, dimmed: false)
        case .downloading(let fraction): GlyphFrame(progress: fraction.map(GlyphFrame.index), dimmed: false)
        case .paused(let fraction): GlyphFrame(progress: fraction.map(GlyphFrame.index), dimmed: true)
        case .completed: GlyphFrame(progress: GlyphFrame.count - 1, dimmed: false)
        }
    }
}

/// One of the pictures make-icons.py draws: the plain icon when progress is
/// nil, otherwise that many of the arrow's 25 chunks filled in. Dimmed is
/// the paused look.
struct GlyphFrame: Equatable {
    static let count = 26
    let progress: Int?
    let dimmed: Bool

    static func index(_ fraction: Double) -> Int {
        Int((min(max(fraction, 0), 1) * Double(count - 1)).rounded(.down))
    }
}
