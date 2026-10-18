import AppKit
import OpenDMCore
import SwiftUI

/// IDM's "Refresh download address": an expired download gets a new
/// address from the browser. The page it came from opens, the user starts
/// the same download there again, and the next download the browser hands
/// over is offered as the new address. The old download then continues from
/// the part already on disk.
@MainActor
enum AddressRefresh {
    private static var panel: WaitingForAddressController?

    static func start(id: String, core: DownloadCore) {
        guard let row = NewDownloads.store?.row(id) else { return }
        let page: String?
        do {
            page = try core.waitForAddress(id: id)
        } catch {
            let alert = NSAlert()
            alert.messageText = "This download cannot get a new address"
            alert.informativeText = error.message
            alert.runModal()
            return
        }
        panel?.finish(cancelled: false)
        let controller = WaitingForAddressController(row: row, page: page) {
            core.stopWaitingForAddress()
            panel = nil
        }
        panel = controller
        controller.present()
        if let page, let url = URL(string: page) {
            NSWorkspace.shared.open(url)
        }
    }

    /// The browser handed over a download while OpenDM was waiting.
    static func received(id: String, download: NewDownload, core: DownloadCore) {
        panel?.finish(cancelled: false)
        panel = nil
        guard let row = NewDownloads.store?.row(id) else {
            NewDownloads.present([download], core: core)
            return
        }
        let name = core.planDownload(download: download).name
        var details = ["New: \(name)" + (download.size.map { " (\(Format.bytes($0)))" } ?? ""), download.url]
        if let size = download.size, size > 0, row.totalBytes > 0, size != row.totalBytes {
            details.append("Its size differs from the download's (\(Format.bytes(row.totalBytes))): it may be another file.")
        }
        let alert = NSAlert()
        alert.messageText = "Is this the new address for “\(row.name)”?"
        alert.informativeText = details.joined(separator: "\n")
        alert.addButton(withTitle: "Use This Address")
        alert.addButton(withTitle: "Download It Separately")
        NSApp.activate(ignoringOtherApps: true)
        if alert.runModal() == .alertFirstButtonReturn {
            core.refreshAddress(id: id, download: download)
            ProgressWindowController.show(ids: [id], core: core)
        } else {
            NewDownloads.present([download], core: core)
        }
    }
}

/// Says what to do while OpenDM waits for the new address.
@MainActor
final class WaitingForAddressController: ConfirmationPanelController {
    private var onCancel: (() -> Void)?

    init(row: TaskRow, page: String?, onCancel: @escaping () -> Void) {
        self.onCancel = onCancel
        weak var controller: WaitingForAddressController?
        let view = WaitingForAddressView(row: row, page: page) { controller?.finish(cancelled: true) }
        super.init(title: "Refresh Download Address", content: view)
        controller = self
        // It waits beside the browser, which needs the screen now.
        window?.level = .normal
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    func finish(cancelled: Bool) {
        if cancelled { onCancel?() }
        onCancel = nil
        close()
    }

    /// The close button is Cancel.
    override func windowWillClose(_ notification: Notification) {
        onCancel?()
        onCancel = nil
        super.windowWillClose(notification)
    }
}

struct WaitingForAddressView: View {
    let row: TaskRow
    let page: String?
    let cancel: () -> Void

    private var progress: String {
        guard row.doneBytes > 0 else { return "from the start" }
        let total = row.totalBytes > 0 ? " of \(Format.bytes(row.totalBytes))" : ""
        return "from \(Format.bytes(row.doneBytes))\(total)"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(alignment: .top, spacing: 14) {
                Image(nsImage: FileIcons.icon(for: row.name, size: 44))
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 6) {
                        ProgressView().controlSize(.small)
                        Text("Waiting for a new address for “\(row.name)”")
                            .font(.headline)
                            .lineLimit(2)
                            .truncationMode(.middle)
                    }
                    Text(page == nil
                        ? "Start the same download again in your browser. OpenDM takes its new address and continues \(progress)."
                        : "The page it came from is open in your browser. Start the same download there again, and OpenDM takes its new address and continues \(progress).")
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            HStack {
                Spacer()
                Button("Cancel", action: cancel)
                    .keyboardShortcut(.cancelAction)
            }
            .padding(.top, 6)
        }
        .padding(20)
        .frame(width: 460)
    }
}

