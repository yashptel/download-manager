import Foundation
import OpenDMCore

enum GlyphState: Equatable {
    case idle
    case downloading(fraction: Double)
    case paused
    case completed

    static func from(_ rows: [TaskRow]) -> GlyphState {
        let active = rows.filter { $0.status == .downloading }
        if !active.isEmpty {
            let known = active.filter { $0.totalBytes > 0 }
            let total = known.reduce(0.0) { $0 + Double($1.totalBytes) }
            let done = known.reduce(0.0) { $0 + Double($1.doneBytes) }
            let fraction = total > 0 ? min(max(done / total, 0), 1) : 0
            return .downloading(fraction: fraction)
        }
        return rows.contains { $0.status == .paused } ? .paused : .idle
    }

    var isDownloading: Bool {
        if case .downloading = self { true } else { false }
    }
}

func frameIndex(for state: GlyphState) -> Int {
    switch state {
    case .downloading(let fraction):
        return Int((min(max(fraction, 0), 1) * 25).rounded(.down))
    case .completed:
        return 25
    case .idle, .paused:
        return 0
    }
}
