import Foundation
import OpenDMCore

struct RowChange {
    /// nil for a row that is new to the list.
    let old: TaskRow?
    let new: TaskRow
}

@MainActor
protocol DownloadStoreObserver: AnyObject {
    func store(_ store: DownloadStore, didChange changes: [RowChange])
    func store(_ store: DownloadStore, didRemove ids: Set<String>)
    func storeStatusDidChange(_ store: DownloadStore)
}

extension DownloadStoreObserver {
    func store(_ store: DownloadStore, didChange changes: [RowChange]) {}
    func store(_ store: DownloadStore, didRemove ids: Set<String>) {}
    func storeStatusDidChange(_ store: DownloadStore) {}
}

/// Main-thread copy of the task list. The core sends only the rows that
/// changed; the store merges them and tells observers exactly what changed.
@MainActor
final class DownloadStore {
    /// Oldest first.
    private(set) var rows: [TaskRow] = []
    private var indexByID: [String: Int] = [:]
    private(set) var stats = GlobalStats(downloadSpeed: 0, uploadSpeed: 0, active: 0, waiting: 0)
    private(set) var engineState: EngineState = .stopped
    var onError: ((String) -> Void)?

    private struct WeakObserver { weak var value: DownloadStoreObserver? }
    private var observers: [WeakObserver] = []

    private(set) lazy var listener: CoreListener = Listener(store: self)

    func load(from core: DownloadCore) {
        rows = core.rows()
        reindex()
        stats = core.stats()
        engineState = core.engineState()
    }

    func row(_ id: String) -> TaskRow? {
        indexByID[id].map { rows[$0] }
    }

    func addObserver(_ observer: DownloadStoreObserver) {
        observers.append(WeakObserver(value: observer))
    }

    fileprivate func apply(_ changed: [TaskRow]) {
        var changes: [RowChange] = []
        changes.reserveCapacity(changed.count)
        for row in changed {
            if let index = indexByID[row.id] {
                changes.append(RowChange(old: rows[index], new: row))
                rows[index] = row
            } else {
                indexByID[row.id] = rows.count
                rows.append(row)
                changes.append(RowChange(old: nil, new: row))
            }
        }
        observers.forEach { $0.value?.store(self, didChange: changes) }
    }

    fileprivate func remove(_ ids: [String]) {
        let removed = Set(ids)
        rows.removeAll { removed.contains($0.id) }
        reindex()
        observers.forEach { $0.value?.store(self, didRemove: removed) }
    }

    fileprivate func setStats(_ stats: GlobalStats) {
        self.stats = stats
        observers.forEach { $0.value?.storeStatusDidChange(self) }
    }

    fileprivate func setEngineState(_ state: EngineState) {
        engineState = state
        observers.forEach { $0.value?.storeStatusDidChange(self) }
    }

    fileprivate func report(_ message: String) {
        onError?(message)
    }

    private func reindex() {
        indexByID = Dictionary(uniqueKeysWithValues: rows.enumerated().map { ($1.id, $0) })
    }
}

/// Receives core events on a background thread and replays them, in order,
/// on the main thread.
private final class Listener: CoreListener, @unchecked Sendable {
    private weak var store: DownloadStore?

    init(store: DownloadStore) {
        self.store = store
    }

    func rowsChanged(rows: [TaskRow]) { onMain { $0.apply(rows) } }
    func rowsRemoved(ids: [String]) { onMain { $0.remove(ids) } }
    func statsChanged(stats: GlobalStats) { onMain { $0.setStats(stats) } }
    func engineStateChanged(state: EngineState) { onMain { $0.setEngineState(state) } }
    func error(message: String) { onMain { $0.report(message) } }

    private func onMain(_ body: @escaping @MainActor (DownloadStore) -> Void) {
        DispatchQueue.main.async { [weak self] in
            MainActor.assumeIsolated {
                if let store = self?.store { body(store) }
            }
        }
    }
}
