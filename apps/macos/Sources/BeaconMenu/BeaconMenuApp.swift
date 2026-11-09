import AppKit
import BeaconCore
import SwiftUI

@main
struct BeaconMenuApp: App {
    @StateObject private var model: StatusModel

    init() {
        // LSUIElement covers the bundled app; this also hides the Dock icon
        // when run unbundled with `swift run`.
        NSApplication.shared.setActivationPolicy(.accessory)
        let model = StatusModel()
        model.start()
        _model = StateObject(wrappedValue: model)
    }

    var body: some Scene {
        MenuBarExtra {
            StatusView(model: model)
        } label: {
            Image(nsImage: model.icon.image)
                .accessibilityLabel(model.icon.label)
        }
        .menuBarExtraStyle(.window)
    }
}

struct StatusView: View {
    @ObservedObject var model: StatusModel
    private let maxProblems = 6

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            header
            Divider()
            content
            Divider()
            footer
        }
        .padding(12)
        .frame(width: 320)
    }

    private var header: some View {
        HStack(alignment: .top, spacing: 8) {
            Image(nsImage: model.icon.image)
            VStack(alignment: .leading, spacing: 2) {
                Text(model.snapshot?.title ?? "Beacon").font(.headline)
                Text(model.icon.label)
                if let breakdown = model.snapshot?.breakdown, !breakdown.isEmpty {
                    Text(breakdown).font(.caption).foregroundStyle(.secondary)
                }
            }
        }
    }

    @ViewBuilder
    private var content: some View {
        if let snapshot = model.snapshot {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(snapshot.publishers, id: \.id) { publisher in
                    publisherRow(publisher)
                }
            }
        } else if let error = model.error {
            Text(error)
                .font(.caption)
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
        } else {
            ProgressView().controlSize(.small)
        }
    }

    private func publisherRow(_ publisher: PublisherStatus) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                Circle()
                    .fill(PublisherPresentation.tone(publisher).color)
                    .frame(width: 8, height: 8)
                Text(publisher.name).fontWeight(.medium)
                Spacer()
                Text(PublisherPresentation.summary(publisher))
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            ForEach(publisher.problems.prefix(maxProblems), id: \.id) { problem in
                Text(PublisherPresentation.problem(problem))
                    .font(.caption)
                    .foregroundStyle(PublisherPresentation.tone(problem).color)
                    .padding(.leading, 14)
            }
            if publisher.problems.count > maxProblems {
                Text("and \(publisher.problems.count - maxProblems) more")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.leading, 14)
            }
        }
    }

    private var footer: some View {
        HStack {
            if let lastAttempt = model.lastAttempt {
                Text("Updated \(lastAttempt, style: .relative) ago")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Button {
                Task { await model.refresh() }
            } label: {
                Image(systemName: "arrow.clockwise")
            }
            .help("Refresh")
            .disabled(model.isRefreshing)
            Button {
                if let url = URL(string: model.pageURL) {
                    NSWorkspace.shared.open(url)
                }
            } label: {
                Image(systemName: "safari")
            }
            .help("Open status page")
            Button("Quit") { NSApplication.shared.terminate(nil) }
                .keyboardShortcut("q")
        }
    }
}
