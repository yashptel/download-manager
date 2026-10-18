import AppKit

/// The three list designs being compared. Switch with the toolbar control or
/// View > as Cards / as List / as Columns (⌘1–⌘3).
enum ListStyle: Int, CaseIterable {
    case cards, list, columns

    var title: String {
        switch self {
        case .cards: "Cards"
        case .list: "List"
        case .columns: "Columns"
        }
    }

    var symbol: String {
        switch self {
        case .cards: "rectangle.grid.1x2"
        case .list: "list.bullet"
        case .columns: "tablecells"
        }
    }

    var rowHeight: CGFloat {
        switch self {
        case .cards: 104
        case .list: 54
        case .columns: 26
        }
    }

    static var saved: ListStyle {
        get { ListStyle(rawValue: UserDefaults.standard.integer(forKey: "ListStyle")) ?? .cards }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: "ListStyle") }
    }
}

/// What a row's own buttons ask the list to do.
enum RowAction {
    /// Pause, resume, retry or open, depending on the status.
    case primary
    case reveal
    case copyAddress
    case remove
}
