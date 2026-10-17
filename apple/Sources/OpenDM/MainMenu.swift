import AppKit

/// The menu bar. Commands use the responder chain, so they reach the main
/// window controller (selection commands) or the app delegate.
@MainActor
enum MainMenu {
    static func build() -> NSMenu {
        let main = NSMenu()
        main.addItem(submenu("OpenDM", [
            item("About OpenDM", #selector(NSApplication.orderFrontStandardAboutPanel(_:))),
            .separator(),
            item("Hide OpenDM", #selector(NSApplication.hide(_:)), "h"),
            item("Hide Others", #selector(NSApplication.hideOtherApplications(_:)), "h", [.command, .option]),
            item("Show All", #selector(NSApplication.unhideAllApplications(_:))),
            .separator(),
            item("Quit OpenDM", #selector(NSApplication.terminate(_:)), "q"),
        ]))
        main.addItem(submenu("File", [
            item("New Download…", #selector(MainWindowController.newDownload(_:)), "n"),
            .separator(),
            item("Close", #selector(NSWindow.performClose(_:)), "w"),
        ]))
        main.addItem(submenu("Edit", [
            item("Cut", #selector(NSText.cut(_:)), "x"),
            item("Copy", #selector(NSText.copy(_:)), "c"),
            item("Paste", #selector(NSText.paste(_:)), "v"),
            item("Select All", #selector(NSText.selectAll(_:)), "a"),
        ]))
        main.addItem(submenu("Downloads", [
            item("Open", #selector(MainWindowController.openSelected(_:)), String(UnicodeScalar(NSDownArrowFunctionKey)!)),
            item("Show in Finder", #selector(MainWindowController.revealSelected(_:)), "r"),
            .separator(),
            item("Resume", #selector(MainWindowController.resumeSelected(_:)), "/"),
            item("Pause", #selector(MainWindowController.pauseSelected(_:)), "."),
            item("Resume All", #selector(AppDelegate.resumeAll(_:)), "/", [.command, .option]),
            item("Pause All", #selector(AppDelegate.pauseAll(_:)), ".", [.command, .option]),
            .separator(),
            item("Remove", #selector(MainWindowController.removeSelected(_:)), "\u{8}"),
            item("Remove and Delete Files…", #selector(MainWindowController.removeSelectedDeletingFiles(_:)), "\u{8}",
                 [.command, .option]),
        ]))
        main.addItem(submenu("View", [
            item("Show Sidebar", #selector(NSSplitViewController.toggleSidebar(_:)), "s", [.command, .control]),
            item("Enter Full Screen", #selector(NSWindow.toggleFullScreen(_:)), "f", [.command, .control]),
        ]))
        let window = submenu("Window", [
            item("Minimize", #selector(NSWindow.performMiniaturize(_:)), "m"),
            item("Zoom", #selector(NSWindow.performZoom(_:))),
            .separator(),
            item("Downloads", #selector(AppDelegate.showMainWindow(_:)), "0"),
            .separator(),
            item("Bring All to Front", #selector(NSApplication.arrangeInFront(_:))),
        ])
        main.addItem(window)
        NSApp.windowsMenu = window.submenu
        return main
    }

    private static func item(_ title: String, _ action: Selector, _ key: String = "",
                             _ modifiers: NSEvent.ModifierFlags = .command) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
        item.keyEquivalentModifierMask = modifiers
        return item
    }

    private static func submenu(_ title: String, _ items: [NSMenuItem]) -> NSMenuItem {
        let menu = NSMenu(title: title)
        items.forEach(menu.addItem)
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.submenu = menu
        return item
    }
}
