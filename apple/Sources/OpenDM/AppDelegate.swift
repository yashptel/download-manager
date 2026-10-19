import AppKit
import OpenDMCore

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var core: DownloadCore?
    private let store = DownloadStore()
    private var mainWindow: MainWindowController?
    private var statusItem: StatusItemController?
    private var updateCheck: Task<Void, Never>?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.mainMenu = MainMenu.build()
        let core: DownloadCore
        do {
            core = try DownloadCore(config: Self.config(), listener: store.listener)
        } catch {
            let alert = NSAlert()
            alert.alertStyle = .critical
            alert.messageText = "OpenDM could not open its data"
            alert.informativeText = error.message
            alert.runModal()
            NSApp.terminate(nil)
            return
        }
        self.core = core
        store.load(from: core)
        ProgressWindowController.store = store
        NewDownloads.store = store
        store.onNewAddress = { [weak self] id, download in
            guard let core = self?.core else { return }
            AddressRefresh.received(id: id, download: download, core: core)
        }
        store.onLoginNeeded = { [weak self] id, url in
            guard let core = self?.core else { return }
            LoginPrompt.handle(id: id, url: url, core: core)
        }
        store.onBrowserDownloads = { [weak self] downloads, _ in
            guard let core = self?.core else { return }
            NewDownloads.present(downloads, core: core)
        }
        mainWindow = MainWindowController(store: store, core: core)
        statusItem = StatusItemController(store: store)
        core.start()
        Self.installBrowserHost(core)
        // At launch, then daily, unless turned off in Settings.
        updateCheck = Task { [weak self] in
            while !Task.isCancelled {
                if Updates.automatic, let release = try? await Updates.check() {
                    self?.mainWindow?.showUpdate(release)
                }
                try? await Task.sleep(for: .seconds(86_400))
            }
        }
        // At login, and when the browser extension starts it (--background),
        // OpenDM opens in the menu bar only.
        if CommandLine.arguments.contains("--background") || Self.launchedAtLogin() {
            NSApp.setActivationPolicy(.accessory)
        } else {
            mainWindow?.show()
        }
    }

    /// Whether macOS opened the app as a login item.
    private static func launchedAtLogin() -> Bool {
        guard let event = NSAppleEventManager.shared().currentAppleEvent else { return false }
        return event.eventID == kAEOpenApplication
            && event.paramDescriptor(forKeyword: keyAEPropData)?.enumCodeValue == keyAELaunchedAsLogInItem
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

    @objc func showSettings(_ sender: Any?) {
        guard let core else { return }
        SettingsWindowController.show(core: core)
    }

    /// From the menu bar item, with or without the window open.
    @objc func newDownload(_ sender: Any?) {
        mainWindow?.newDownload(sender)
    }

    @objc func pauseAll(_ sender: Any?) {
        core?.pauseAll()
    }

    @objc func resumeAll(_ sender: Any?) {
        core?.resumeAll()
    }

    @objc func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        switch menuItem.action {
        case #selector(pauseAll(_:)): store.rows.contains(where: \.canPause)
        case #selector(resumeAll(_:)): store.rows.contains { $0.status == .paused }
        default: true
        }
    }

    /// Lets installed browsers start the bundled opendm-host, which connects
    /// the extension to this app. Rewritten at each launch in case the app moved.
    private static func installBrowserHost(_ core: DownloadCore) {
        guard let host = Bundle.main.url(forAuxiliaryExecutable: "opendm-host")?.path else { return }
        DispatchQueue.global(qos: .utility).async {
            do {
                _ = try core.installBrowserHost(hostPath: host)
            } catch {
                NSLog("OpenDM: could not register the browser host: %@", error.message)
            }
        }
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
