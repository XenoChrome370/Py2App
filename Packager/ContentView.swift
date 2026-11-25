import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @StateObject private var session = BuildSession()
    @State private var dropTargeted = false

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                if let banner = session.banner {
                    bannerBar(banner)
                        .padding(.horizontal, 16)
                        .padding(.top, 12)
                }
                HSplitView {
                    ScrollView {
                        form
                            .padding(16)
                    }
                    .frame(minWidth: 360, idealWidth: 420, maxWidth: 480)
                    logPane
                        .frame(minWidth: 380)
                }
            }
            .navigationTitle("Packager")
            .navigationSubtitle("Build a project into an app")
            .toolbar {
                toolbar
            }
            .overlay {
                if dropTargeted {
                    RoundedRectangle(cornerRadius: 10)
                        .stroke(Color.accentColor, lineWidth: 2)
                        .padding(6)
                        .allowsHitTesting(false)
                }
            }
        }
        .task { await session.bootstrap() }
        .onChange(of: session.sourcePath) { _, _ in
            session.persist()
            session.scheduleDetect()
        }
        .onChange(of: session.language) { _, _ in
            session.persist()
            session.scheduleDetect()
        }
        .onChange(of: session.appName) { _, _ in
            session.noteNameEdit()
        }
        .onChange(of: session.mode) { _, _ in session.persist() }
        .onDrop(of: [.fileURL], isTargeted: $dropTargeted) { providers in
            loadDrop(providers)
        }
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            if session.running {
                Button("Stop") { session.stop() }
            }
            Button(session.running ? "Building…" : "Build") {
                session.start()
            }
            .keyboardShortcut(.defaultAction)
            .disabled(!session.canBuild)
        }
    }

    private var form: some View {
        Form {
            Section("Project") {
                LabeledContent("Source") {
                    HStack(spacing: 8) {
                        TextField("File or project folder", text: $session.sourcePath)
                            .textFieldStyle(.roundedBorder)
                        Button("Browse") { session.chooseSource() }
                    }
                }
                if let detection = session.detection, detection.ok {
                    LabeledContent("Detected") {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(detection.label ?? detection.language ?? "Project")
                            if let reason = detection.reason {
                                Text(reason)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            if let entry = detection.entry {
                                Text(entry)
                                    .font(.caption)
                                    .foregroundStyle(.tertiary)
                                    .lineLimit(2)
                                    .truncationMode(.middle)
                            }
                        }
                    }
                } else if let detectError = session.detectError {
                    Text(detectError)
                        .font(.callout)
                        .foregroundStyle(.orange)
                }
            }

            Section {
                LabeledContent("Name") {
                    TextField("App name", text: $session.appName)
                        .textFieldStyle(.roundedBorder)
                }
                LabeledContent("Save to") {
                    HStack(spacing: 8) {
                        TextField("Output folder", text: $session.outputPath)
                            .textFieldStyle(.roundedBorder)
                        Button("Browse") { session.chooseOutput() }
                    }
                }
                Picker("Product", selection: $session.mode) {
                    Text("App").tag("app")
                    Text("Tool").tag("tool")
                }
                .pickerStyle(.segmented)
                LabeledContent("Icon") {
                    HStack(spacing: 8) {
                        TextField("Optional .icns or .png", text: $session.iconPath)
                            .textFieldStyle(.roundedBorder)
                        Button("Browse") { session.chooseIcon() }
                        if !session.iconPath.isEmpty {
                            Button("Clear") { session.clearIcon() }
                        }
                    }
                }
            } header: {
                Text("Output")
            } footer: {
                Text("App is a double-clickable macOS application. Tool is a program for the terminal. Packaging uses the compilers installed on this Mac.")
            }

            Section("Language") {
                Picker("Language", selection: $session.language) {
                    Text("Automatic").tag("auto")
                    ForEach(session.languages) { language in
                        Text(language.label).tag(language.id)
                    }
                }
                if let tool = session.activeToolchain {
                    Label {
                        Text(tool.detail)
                            .font(.caption)
                            .foregroundStyle(tool.available ? Color.secondary : Color.orange)
                    } icon: {
                        Image(systemName: tool.available ? "checkmark.circle.fill" : "exclamationmark.circle")
                            .foregroundStyle(tool.available ? Color.green : Color.orange)
                    }
                }
            }

            Section {
                if session.loading {
                    ProgressView()
                        .controlSize(.small)
                } else if let toolchainError = session.toolchainError {
                    Text(toolchainError)
                        .foregroundStyle(.orange)
                } else if session.engineMissing {
                    Text("The Rust packaging engine is missing from this app.")
                        .foregroundStyle(.orange)
                } else {
                    ForEach(session.tools) { tool in
                        HStack(spacing: 8) {
                            Image(systemName: tool.available ? "circle.fill" : "circle")
                                .font(.system(size: 8))
                                .foregroundStyle(tool.available ? Color.green : Color.secondary)
                            Text(tool.label)
                            Spacer()
                            Text(tool.available ? tool.tool : "Not installed")
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                        .help(tool.detail)
                    }
                }
            } header: {
                HStack {
                    Text("Toolchains")
                    Spacer()
                    Button("Refresh") {
                        Task { await session.refreshToolchains() }
                    }
                    .buttonStyle(.borderless)
                    .disabled(session.loading || session.engineMissing)
                }
            }
        }
        .formStyle(.grouped)
    }

    private var logPane: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Log")
                    .font(.headline)
                Spacer()
                Button("Copy") { session.copyLog() }
                    .disabled(session.logText.isEmpty)
            }
            .padding(.horizontal, 4)
            ScrollViewReader { proxy in
                ScrollView {
                    Text(session.logText.isEmpty ? "Build output will appear here." : session.logText)
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundStyle(session.logText.isEmpty ? .secondary : .primary)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .topLeading)
                        .padding(12)
                    Color.clear.frame(height: 1).id("log-end")
                }
                .background(Color(nsColor: .textBackgroundColor))
                .clipShape(RoundedRectangle(cornerRadius: 8))
                .overlay(
                    RoundedRectangle(cornerRadius: 8)
                        .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
                )
                .onChange(of: session.logText) { _, _ in
                    proxy.scrollTo("log-end", anchor: .bottom)
                }
            }
        }
        .padding(16)
    }

    private func bannerBar(_ banner: StatusBanner) -> some View {
        HStack(spacing: 10) {
            Image(systemName: banner.success ? "checkmark.circle.fill" : "exclamationmark.octagon.fill")
                .foregroundStyle(banner.success ? Color.green : Color.orange)
            Text(banner.message)
                .lineLimit(2)
            Spacer()
            if let artifact = banner.artifact {
                Button("Show in Finder") { session.reveal(artifact) }
            }
        }
        .padding(10)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
    }

    private func loadDrop(_ providers: [NSItemProvider]) -> Bool {
        guard let provider = providers.first(where: {
            $0.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier)
        }) else {
            return false
        }
        provider.loadItem(forTypeIdentifier: UTType.fileURL.identifier, options: nil) { item, _ in
            let url: URL?
            if let data = item as? Data {
                url = URL(dataRepresentation: data, relativeTo: nil)
            } else if let urlItem = item as? URL {
                url = urlItem
            } else if let urlItem = item as? NSURL {
                url = urlItem as URL
            } else {
                url = nil
            }
            guard let url else { return }
            Task { @MainActor in
                let path = url.path
                let changed = session.sourcePath != path
                session.sourcePath = path
                session.nameTouched = false
                session.persist()
                if !changed {
                    session.scheduleDetect()
                }
            }
        }
        return true
    }
}
