import BeaconCore
import Foundation

/// Polls the status page through the shared Rust core. All status logic
/// (freshness, combining locations, headlines) lives in `beacon-core`.
@MainActor
final class StatusModel: ObservableObject {
    static let refreshInterval: Duration = .seconds(60)
    /// Optional override: `defaults write uk.guzek.beacon.menu statusPageURL <url>`.
    static let pageURLKey = "statusPageURL"

    @Published private(set) var snapshot: Snapshot?
    @Published private(set) var error: String?
    @Published private(set) var isRefreshing = false
    @Published private(set) var lastAttempt: Date?

    let pageURL: String
    private var loop: Task<Void, Never>?

    init(pageURL: String = StatusModel.configuredPageURL()) {
        self.pageURL = pageURL
    }

    nonisolated static func configuredPageURL(defaults: UserDefaults = .standard) -> String {
        let value = defaults.string(forKey: pageURLKey)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        if let value, !value.isEmpty {
            return value
        }
        return defaultStatusPageUrl()
    }

    var icon: MenuBarIcon {
        MenuBarIcon.make(snapshot: snapshot, error: error)
    }

    func start() {
        guard loop == nil else { return }
        loop = Task { [weak self] in
            while !Task.isCancelled {
                guard let model = self else { return }
                await model.refresh()
                try? await Task.sleep(for: StatusModel.refreshInterval)
            }
        }
    }

    func refresh() async {
        guard !isRefreshing else { return }
        isRefreshing = true
        defer { isRefreshing = false }
        let url = pageURL
        // fetchStatus blocks on network I/O, so keep it off the main actor.
        let result = await Task.detached(priority: .utility) { () -> Result<Snapshot, Error> in
            do {
                return .success(try fetchStatus(pageUrl: url))
            } catch {
                return .failure(error)
            }
        }.value
        lastAttempt = Date()
        switch result {
        case .success(let value):
            snapshot = value
            error = nil
        case .failure(let failure):
            // Don't keep showing results we can no longer confirm.
            snapshot = nil
            error = failure.localizedDescription
        }
    }
}
