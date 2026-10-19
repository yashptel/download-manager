import AppKit

/// Asks GitHub for the latest release, as the Windows app does. OpenDM checks
/// when it starts and once a day while it runs, unless the user turned that
/// off in Settings; Settings can also check at any time. OPENDM_UPDATE_URL
/// points the check elsewhere for testing.
@MainActor
enum Updates {
    /// A published version and the page to download it from.
    struct Release {
        let version: String
        let page: URL
    }

    enum CheckError: LocalizedError {
        case noRelease
        case server(Int)

        var errorDescription: String? {
            switch self {
            case .noRelease: "No release has been published yet."
            case .server(let status): "GitHub answered with HTTP \(status)."
            }
        }
    }

    private static let latestRelease = URL(string: "https://api.github.com/repos/yashptel/download-manager/releases/latest")!

    /// This copy's version, from the build (Cargo.toml).
    static var current: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0.0"
    }

    static var automatic: Bool {
        get { UserDefaults.standard.object(forKey: "CheckForUpdates") as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: "CheckForUpdates") }
    }

    /// The latest release when it is newer than this copy, otherwise nil.
    static func check() async throws -> Release? {
        let url = ProcessInfo.processInfo.environment["OPENDM_UPDATE_URL"].flatMap(URL.init(string:)) ?? latestRelease
        var request = URLRequest(url: url, timeoutInterval: 20)
        request.setValue("OpenDM/\(current)", forHTTPHeaderField: "User-Agent")
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        let (data, response) = try await URLSession.shared.data(for: request)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        if status == 404 { throw CheckError.noRelease }
        guard (200..<300).contains(status) else { throw CheckError.server(status) }
        struct Latest: Decodable {
            let tagName: String
            let htmlURL: URL
            enum CodingKeys: String, CodingKey {
                case tagName = "tag_name"
                case htmlURL = "html_url"
            }
        }
        let latest = try JSONDecoder().decode(Latest.self, from: data)
        guard let version = parse(latest.tagName), let mine = parse(current), isNewer(version, than: mine) else {
            return nil
        }
        return Release(version: version.map(String.init).joined(separator: "."), page: latest.htmlURL)
    }

    /// "v1.2.3", "1.2.3+commit" or "1.2.3-beta" as [1, 2, 3].
    private static func parse(_ text: String) -> [Int]? {
        let plain = text.drop { $0 == "v" || $0 == "V" }.split { $0 == "+" || $0 == "-" }.first ?? ""
        let parts = plain.split(separator: ".").map { Int($0) }
        guard !parts.isEmpty, parts.allSatisfy({ $0 != nil }) else { return nil }
        return parts.compactMap { $0 }
    }

    private static func isNewer(_ a: [Int], than b: [Int]) -> Bool {
        for index in 0..<max(a.count, b.count) {
            let (x, y) = (index < a.count ? a[index] : 0, index < b.count ? b[index] : 0)
            if x != y { return x > y }
        }
        return false
    }
}

