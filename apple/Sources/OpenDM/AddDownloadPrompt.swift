import AppKit
import OpenDMCore

/// Asks for one or more links, prefilled with links on the clipboard, then
/// confirms them like any new download.
@MainActor
enum AddDownloadPrompt {
    static func run(in window: NSWindow, core: DownloadCore) {
        let alert = NSAlert()
        alert.messageText = "New Download"
        alert.informativeText = "Enter one or more addresses, one per line."
        let scrollView = NSTextView.scrollableTextView()
        scrollView.frame = NSRect(x: 0, y: 0, width: 440, height: 96)
        scrollView.borderType = .bezelBorder
        let textView = scrollView.documentView as! NSTextView
        textView.isRichText = false
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticLinkDetectionEnabled = false
        textView.font = .systemFont(ofSize: NSFont.systemFontSize)
        textView.textContainerInset = NSSize(width: 3, height: 5)
        textView.string = links(in: NSPasteboard.general.string(forType: .string) ?? "").joined(separator: "\n")
        textView.selectAll(nil)
        // The alert sets its accessory wider than its text; the box is inset
        // to line up with the text on both sides.
        let box = NSView(frame: NSRect(x: 0, y: 0, width: 440, height: 96))
        scrollView.frame = NSRect(x: 6, y: 0, width: 428, height: 96)
        box.addSubview(scrollView)
        alert.accessoryView = box
        alert.addButton(withTitle: "Download")
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = textView
        alert.beginSheetModal(for: window) { response in
            let urls = links(in: textView.string)
            guard response == .alertFirstButtonReturn, !urls.isEmpty else { return }
            NewDownloads.present(urls.map { NewDownload(url: $0) }, core: core)
        }
    }

    /// Every whitespace-separated token that looks like a download link.
    static func links(in text: String) -> [String] {
        text.split(whereSeparator: \.isWhitespace).map(String.init).filter { token in
            let lower = token.lowercased()
            return ["http://", "https://", "sftp://", "magnet:?"].contains { lower.hasPrefix($0) }
        }
    }
}
