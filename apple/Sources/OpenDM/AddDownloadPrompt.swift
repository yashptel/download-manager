import AppKit
import OpenDMCore

/// Asks for one or more links, prefilled with links on the clipboard.
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
        textView.string = links(in: NSPasteboard.general.string(forType: .string) ?? "").joined(separator: "\n")
        textView.selectAll(nil)
        alert.accessoryView = scrollView
        alert.addButton(withTitle: "Start")
        alert.addButton(withTitle: "Download Later")
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = textView
        alert.beginSheetModal(for: window) { response in
            let urls = links(in: textView.string)
            guard !urls.isEmpty else { return }
            switch response {
            case .alertFirstButtonReturn: core.addUrls(urls: urls, startPaused: false)
            case .alertSecondButtonReturn: core.addUrls(urls: urls, startPaused: true)
            default: break
            }
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
