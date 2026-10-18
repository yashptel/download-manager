import AppKit
import OpenDMCore
import Security
import SwiftUI

/// A saved user name for a site that asks for one (HTTP 401). The site is a
/// host, optionally with a path ("example.com/private") to cover only that
/// part of it. The password stays in the keychain.
struct SiteLogin: Hashable, Identifiable {
    let site: String
    let user: String
    var id: String { site + "\n" + user }
}

/// Site logins in the user's login keychain, as generic passwords of one
/// service. The account holds the site and the user name.
@MainActor
enum SiteLogins {
    private static let service = "OpenDM site login"

    static func all() -> [SiteLogin] {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecReturnAttributes as String: true,
            kSecMatchLimit as String: kSecMatchLimitAll,
        ]
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let items = result as? [[String: Any]] else { return [] }
        return items
            .compactMap { ($0[kSecAttrAccount as String] as? String).flatMap(login(account:)) }
            .sorted { ($0.site, $0.user) < ($1.site, $1.user) }
    }

    static func password(for login: SiteLogin) -> String? {
        var query = itemQuery(login)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let data = result as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    /// Saves or replaces the password of a site and user name.
    static func save(_ login: SiteLogin, password: String) throws {
        SecItemDelete(itemQuery(login) as CFDictionary)
        var item = itemQuery(login)
        item[kSecValueData as String] = Data(password.utf8)
        item[kSecAttrLabel as String] = "OpenDM: \(login.site)"
        let status = SecItemAdd(item as CFDictionary, nil)
        guard status == errSecSuccess else {
            throw NSError(domain: NSOSStatusErrorDomain, code: Int(status), userInfo: [
                NSLocalizedDescriptionKey: (SecCopyErrorMessageString(status, nil) as String?) ?? "Keychain error \(status)",
            ])
        }
    }

    static func remove(_ login: SiteLogin) {
        SecItemDelete(itemQuery(login) as CFDictionary)
    }

    /// The saved login that covers `url` most closely: the same host, and
    /// the longest path that starts the URL's path.
    static func match(for url: String) -> SiteLogin? {
        guard let components = URLComponents(string: url), let host = components.host?.lowercased() else { return nil }
        let path = components.path
        return all()
            .filter { login in
                let (siteHost, sitePath) = split(login.site)
                return siteHost == host && (sitePath.isEmpty || path.hasPrefix(sitePath))
            }
            .max { $0.site.count < $1.site.count }
    }

    /// "example.com/private/" from what was typed, for storing and matching.
    static func normalize(_ site: String) -> String {
        var text = site.trimmingCharacters(in: .whitespacesAndNewlines)
        if let scheme = text.range(of: "://") { text = String(text[scheme.upperBound...]) }
        let (host, path) = split(text)
        return host.lowercased() + path
    }

    /// The host of a URL, which a remembered login covers.
    static func site(of url: String) -> String {
        URLComponents(string: url)?.host?.lowercased() ?? url
    }

    private static func split(_ site: String) -> (host: String, path: String) {
        guard let slash = site.firstIndex(of: "/") else { return (site, "") }
        return (String(site[..<slash]), String(site[slash...]))
    }

    private static func itemQuery(_ login: SiteLogin) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: login.user + "\n" + login.site,
        ]
    }

    private static func login(account: String) -> SiteLogin? {
        guard let newline = account.lastIndex(of: "\n") else { return nil }
        return SiteLogin(site: String(account[account.index(after: newline)...]), user: String(account[..<newline]))
    }
}

/// What happens when a server asks for a login: a saved one is tried first,
/// once per download; otherwise, or when it fails, the user is asked.
@MainActor
enum LoginPrompt {
    /// Downloads that have had a saved or typed login since the last ask.
    private static var tried: Set<String> = []
    private static var asking: Set<String> = []

    static func handle(id: String, url: String, core: DownloadCore) {
        let saved = SiteLogins.match(for: url)
        if let saved, !tried.contains(id), let password = SiteLogins.password(for: saved) {
            tried.insert(id)
            core.setLogin(id: id, user: saved.user, password: password)
            return
        }
        guard !asking.contains(id) else { return }
        asking.insert(id)
        let name = NewDownloads.store?.row(id)?.name ?? url
        LoginPanelController.show(url: url, name: name, user: saved?.user ?? "") { answer in
            asking.remove(id)
            guard let answer else { return }
            tried.insert(id)
            if answer.remember {
                do {
                    try SiteLogins.save(SiteLogin(site: SiteLogins.site(of: url), user: answer.user), password: answer.password)
                } catch {
                    NSAlert(error: error).runModal()
                }
            }
            core.setLogin(id: id, user: answer.user, password: answer.password)
        }
    }
}

struct LoginAnswer {
    let user: String
    let password: String
    let remember: Bool
}

/// IDM's "Authorization Required": a user name and password for a download.
@MainActor
final class LoginPanelController: ConfirmationPanelController {
    private var done: ((LoginAnswer?) -> Void)?

    static func show(url: String, name: String, user: String, done: @escaping (LoginAnswer?) -> Void) {
        LoginPanelController(url: url, name: name, user: user, done: done).present()
    }

    private init(url: String, name: String, user: String, done: @escaping (LoginAnswer?) -> Void) {
        self.done = done
        weak var controller: LoginPanelController?
        let view = LoginView(host: SiteLogins.site(of: url), name: name, user: user) { controller?.finish($0) }
        super.init(title: "Login Required", content: view)
        controller = self
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    private func finish(_ answer: LoginAnswer?) {
        done?(answer)
        done = nil
        close()
    }

    /// The close button is Cancel.
    override func windowWillClose(_ notification: Notification) {
        done?(nil)
        done = nil
        super.windowWillClose(notification)
    }
}

struct LoginView: View {
    let host: String
    let name: String
    @State var user: String
    @State private var password = ""
    @State private var remember = true
    let finish: (LoginAnswer?) -> Void

    init(host: String, name: String, user: String, finish: @escaping (LoginAnswer?) -> Void) {
        self.host = host
        self.name = name
        _user = State(initialValue: user)
        self.finish = finish
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(alignment: .top, spacing: 14) {
                Image(systemName: "lock.fill")
                    .font(.system(size: 30))
                    .foregroundStyle(.secondary)
                    .frame(width: 44)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 3) {
                    Text("\(host) asks for a user name and password")
                        .font(.headline)
                        .fixedSize(horizontal: false, vertical: true)
                    Text(name)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
            }
            Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 10, verticalSpacing: 10) {
                GridRow {
                    Text("User name:").gridColumnAlignment(.trailing)
                    TextField("User name", text: $user).labelsHidden()
                }
                GridRow {
                    Text("Password:")
                    SecureField("Password", text: $password).labelsHidden()
                }
                GridRow {
                    Color.clear.gridCellUnsizedAxes([.horizontal, .vertical])
                    Toggle("Remember for \(host)", isOn: $remember)
                }
            }
            HStack {
                Spacer()
                Button("Cancel") { finish(nil) }
                    .keyboardShortcut(.cancelAction)
                Button("Log In") {
                    finish(LoginAnswer(user: user.trimmingCharacters(in: .whitespaces), password: password, remember: remember))
                }
                .keyboardShortcut(.defaultAction)
                .disabled(user.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(20)
        .frame(width: 440)
    }
}

