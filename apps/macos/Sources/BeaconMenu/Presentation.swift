import AppKit
import BeaconCore
import SwiftUI

/// Colours from the status page's palette.
enum Tone: Equatable {
    case healthy, mixed, down, unknown

    init(_ state: HealthState) {
        switch state {
        case .healthy: self = .healthy
        case .mixed: self = .mixed
        case .down: self = .down
        case .unknown: self = .unknown
        }
    }

    var nsColor: NSColor {
        switch self {
        case .healthy: return NSColor(srgbRed: 0x7D / 255, green: 0xD3 / 255, blue: 0xB0 / 255, alpha: 1)
        case .mixed: return NSColor(srgbRed: 0xE8 / 255, green: 0xC3 / 255, blue: 0x7A / 255, alpha: 1)
        case .down: return NSColor(srgbRed: 0xED / 255, green: 0x7C / 255, blue: 0x8B / 255, alpha: 1)
        case .unknown: return NSColor(srgbRed: 0x84 / 255, green: 0x91 / 255, blue: 0xA7 / 255, alpha: 1)
        }
    }

    var color: Color { Color(nsColor: nsColor) }
}

/// The menu-bar icon: one symbol per page-level verdict.
struct MenuBarIcon: Equatable {
    let symbol: String
    let tone: Tone
    let label: String

    static let checking = MenuBarIcon(
        symbol: "circle.dotted", tone: .unknown, label: "Checking service status…")
    static let failed = MenuBarIcon(
        symbol: "wifi.exclamationmark", tone: .unknown, label: "Status page unavailable")

    static func make(snapshot: Snapshot?, error: String?) -> MenuBarIcon {
        if let snapshot {
            return make(overview: snapshot.overview, headline: snapshot.headline)
        }
        return error == nil ? .checking : .failed
    }

    static func make(overview: Overview, headline: String) -> MenuBarIcon {
        switch overview {
        case .allOnline:
            return MenuBarIcon(symbol: "checkmark.circle.fill", tone: .healthy, label: headline)
        case .mixed:
            return MenuBarIcon(symbol: "circle.lefthalf.filled", tone: .mixed, label: headline)
        case .partialOutage:
            return MenuBarIcon(symbol: "exclamationmark.triangle.fill", tone: .down, label: headline)
        case .allDown:
            return MenuBarIcon(symbol: "xmark.octagon.fill", tone: .down, label: headline)
        case .incomplete, .unavailable:
            return MenuBarIcon(symbol: "questionmark.circle", tone: .unknown, label: headline)
        }
    }

    /// Menu-bar images are monochrome templates by default; keep the colour.
    var image: NSImage {
        let base =
            NSImage(systemSymbolName: symbol, accessibilityDescription: label)
            ?? NSImage(size: NSSize(width: 16, height: 16))
        let configuration = NSImage.SymbolConfiguration(paletteColors: [tone.nsColor])
        let image = base.withSymbolConfiguration(configuration) ?? base
        image.isTemplate = false
        return image
    }
}

enum PublisherPresentation {
    /// e.g. `"38 online · 2 down"`, or why there is nothing to count.
    static func summary(_ publisher: PublisherStatus) -> String {
        guard publisher.reachable else { return "Unreachable" }
        let parts = [
            (publisher.healthy, "online"),
            (publisher.down, "down"),
            (publisher.unknown, "unknown"),
        ]
        .filter { $0.0 > 0 }
        .map { "\($0.0) \($0.1)" }
        return parts.isEmpty ? "No services" : parts.joined(separator: " · ")
    }

    static func tone(_ publisher: PublisherStatus) -> Tone {
        if !publisher.reachable { return .unknown }
        if publisher.down > 0 { return .down }
        if publisher.unknown > 0 { return .unknown }
        return .healthy
    }

    static func problem(_ problem: ServiceProblem) -> String {
        switch problem.status {
        case .down: return "\(problem.name) — down"
        case .unknown, .healthy: return "\(problem.name) — no recent result"
        }
    }

    static func tone(_ problem: ServiceProblem) -> Tone {
        problem.status == .down ? .down : .unknown
    }
}
