import AppKit
import OpenDMCore

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var core: DownloadCore?
    private let store = DownloadStore()
    private var mainWindow: MainWindowController?
    private var statusItem: StatusItemController?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.mainMenu = MainMenu.build()
        let core: DownloadCore
        do {
            core = try DownloadCore(config: Self.config(), listener: store.listener)
        } catch {
            let alert = NSAlert()
            alert.alertStyle = .critical
            alert.messageText = "OpenDM could not open its data"
            alert.informativeText = error.localizedDescription
            alert.runModal()
            NSApp.terminate(nil)
            return
        }
        self.core = core
        store.load(from: core)
        mainWindow = MainWindowController(store: store, core: core)
        statusItem = StatusItemController(store: store)
        core.start()
        mainWindow?.show()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        mainWindow?.show()
        return true
    }

    func applicationWillTerminate(_ notification: Notification) {
        core?.shutdown()
    }

    @objc func showMainWindow(_ sender: Any?) {
        mainWindow?.show()
    }

    @objc func pauseAll(_ sender: Any?) {
        core?.pauseAll()
    }

    @objc func resumeAll(_ sender: Any?) {
        core?.resumeAll()
    }

    /// Paths can be overridden with OPENDM_DATA_DIR and OPENDM_ENGINE for development.
    private static func config() -> CoreConfig {
        let environment = ProcessInfo.processInfo.environment
        let files = FileManager.default
        let support = files.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("OpenDM")
        let downloads = files.urls(for: .downloadsDirectory, in: .userDomainMask)[0]
        let engine = environment["OPENDM_ENGINE"]
            ?? Bundle.main.url(forAuxiliaryExecutable: "aria2-next")?.path
            ?? ""
        return CoreConfig(
            enginePath: engine,
            dataDir: environment["OPENDM_DATA_DIR"] ?? support.path,
            defaultDownloadDir: downloads.path
        )
    }
}
