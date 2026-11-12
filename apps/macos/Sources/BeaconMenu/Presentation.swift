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

/// What an icon draws: the app's logo, or an SF Symbol.
enum Glyph: Equatable {
    case logo
    /// The logo's left half healthy, its right half a hollow ring in the icon's tone.
    case mixedLogo
    case symbol(String)
}

extension Tone {
    /// The Waybar module's glyphs: ✓ ◐ ✕ ?
    var symbol: String {
        switch self {
        case .healthy: return "checkmark"
        case .mixed: return "circle.lefthalf.filled"
        case .down: return "xmark"
        case .unknown: return "questionmark"
        }
    }
}

extension Glyph {
    func image(tone: Tone, label: String?) -> NSImage {
        let image: NSImage
        switch self {
        case .logo:
            image = Self.logo(left: tone.nsColor, right: tone.nsColor, rightDot: true)
            image.accessibilityDescription = label
        case .mixedLogo:
            image = Self.logo(left: Tone.healthy.nsColor, right: tone.nsColor, rightDot: false)
            image.accessibilityDescription = label
        case .symbol(let name):
            let base =
                NSImage(systemSymbolName: name, accessibilityDescription: label)
                ?? NSImage(size: NSSize(width: 16, height: 16))
            // Bold, like the Waybar text, so the glyphs read at menu-bar size.
            let configuration = NSImage.SymbolConfiguration(pointSize: 13, weight: .bold)
                .applying(NSImage.SymbolConfiguration(paletteColors: [tone.nsColor]))
            image = base.withSymbolConfiguration(configuration) ?? base
        }
        // Menu-bar images are monochrome templates by default; keep the colour.
        image.isTemplate = false
        return image
    }

    /// The favicon (apps/web/public/favicon.svg): a ring around a dot,
    /// drawn as two halves so they can differ.
    private static func logo(left: NSColor, right: NSColor, rightDot: Bool) -> NSImage {
        NSImage(size: NSSize(width: 16, height: 16), flipped: false) { rect in
            let scale = rect.width / 32
            let center = NSPoint(x: rect.midX, y: rect.midY)
            // Counter-clockwise from the top: 90...270 is the left half.
            func half(from start: CGFloat, color: NSColor, dot: Bool) {
                color.set()
                let ring = NSBezierPath()
                ring.appendArc(
                    withCenter: center, radius: 13 * scale, startAngle: start,
                    endAngle: start + 180)
                ring.lineWidth = 3 * scale
                ring.stroke()
                guard dot else { return }
                let semicircle = NSBezierPath()
                semicircle.appendArc(
                    withCenter: center, radius: 7 * scale, startAngle: start,
                    endAngle: start + 180)
                semicircle.close()
                semicircle.fill()
            }
            half(from: 90, color: left, dot: true)
            half(from: 270, color: right, dot: rightDot)
            return true
        }
    }
}

/// The menu-bar icon: one glyph per page-level verdict.
struct MenuBarIcon: Equatable {
    let glyph: Glyph
    let tone: Tone
    let label: String

    static let checking = MenuBarIcon(
        glyph: .logo, tone: .unknown, label: "Checking service status…")
    static let failed = MenuBarIcon(
        glyph: .symbol(Tone.unknown.symbol), tone: .unknown, label: "Status page unavailable")

    static func make(snapshot: Snapshot?, error: String?) -> MenuBarIcon {
        if let snapshot {
            return make(overview: snapshot.overview, headline: snapshot.headline)
        }
        return error == nil ? .checking : .failed
    }

    static func make(overview: Overview, headline: String) -> MenuBarIcon {
        switch overview {
        case .allOnline:
            return MenuBarIcon(glyph: .logo, tone: .healthy, label: headline)
        case .mixed:
            return MenuBarIcon(glyph: .mixedLogo, tone: .mixed, label: headline)
        case .partialOutage, .allDown:
            return MenuBarIcon(tone: .down, label: headline)
        case .incomplete, .unavailable:
            return MenuBarIcon(tone: .unknown, label: headline)
        }
    }

    init(glyph: Glyph, tone: Tone, label: String) {
        self.glyph = glyph
        self.tone = tone
        self.label = label
    }

    /// The tone's own glyph.
    init(tone: Tone, label: String) {
        self.init(glyph: .symbol(tone.symbol), tone: tone, label: label)
    }

    var image: NSImage { glyph.image(tone: tone, label: label) }
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
