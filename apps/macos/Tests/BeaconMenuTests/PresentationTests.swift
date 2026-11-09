import AppKit
import BeaconCore
import XCTest

@testable import BeaconMenu

final class PresentationTests: XCTestCase {
    private func publisher(
        reachable: Bool = true, healthy: UInt32 = 0, down: UInt32 = 0, unknown: UInt32 = 0
    ) -> PublisherStatus {
        PublisherStatus(
            id: "hp", name: "HP", url: "https://beacon-hp.example.org", reachable: reachable,
            generatedAt: reachable ? 100 : nil, healthy: healthy, down: down, unknown: unknown,
            problems: [])
    }

    func testIconFollowsOverview() {
        XCTAssertEqual(MenuBarIcon.make(overview: .allOnline, headline: "").tone, .healthy)
        XCTAssertEqual(MenuBarIcon.make(overview: .mixed, headline: "").tone, .mixed)
        XCTAssertEqual(MenuBarIcon.make(overview: .partialOutage, headline: "").tone, .down)
        XCTAssertEqual(MenuBarIcon.make(overview: .allDown, headline: "").tone, .down)
        XCTAssertEqual(MenuBarIcon.make(overview: .unavailable, headline: "").tone, .unknown)
        XCTAssertEqual(MenuBarIcon.make(snapshot: nil, error: nil), .checking)
        XCTAssertEqual(MenuBarIcon.make(snapshot: nil, error: "offline"), .failed)
    }

    func testIconSymbolsExist() {
        let overviews: [Overview] = [
            .allOnline, .mixed, .partialOutage, .allDown, .incomplete, .unavailable,
        ]
        for icon in overviews.map({ MenuBarIcon.make(overview: $0, headline: "") })
            + [.checking, .failed]
        {
            XCTAssertNotNil(
                NSImage(systemSymbolName: icon.symbol, accessibilityDescription: nil), icon.symbol)
            XCTAssertFalse(icon.image.isTemplate)
        }
    }

    func testPublisherSummary() {
        XCTAssertEqual(
            PublisherPresentation.summary(publisher(healthy: 38, down: 2)), "38 online · 2 down")
        XCTAssertEqual(PublisherPresentation.summary(publisher(reachable: false)), "Unreachable")
        XCTAssertEqual(PublisherPresentation.summary(publisher()), "No services")
        XCTAssertEqual(PublisherPresentation.tone(publisher(healthy: 1, unknown: 1)), .unknown)
        XCTAssertEqual(PublisherPresentation.tone(publisher(healthy: 1, down: 1)), .down)
    }

    func testPageURLOverride() throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "beacon-menu-tests"))
        defaults.removePersistentDomain(forName: "beacon-menu-tests")
        XCTAssertEqual(StatusModel.configuredPageURL(defaults: defaults), defaultStatusPageUrl())
        defaults.set("  https://status.example.org ", forKey: StatusModel.pageURLKey)
        XCTAssertEqual(
            StatusModel.configuredPageURL(defaults: defaults), "https://status.example.org")
    }

    func testRustCoreRejectsInvalidURLWithoutNetwork() {
        XCTAssertThrowsError(try fetchStatus(pageUrl: "ftp://example.org"))
    }
}
