import AppKit
import Foundation
import UniformTypeIdentifiers

struct LanguageChoice: Decodable, Identifiable {
    let id: String
    let label: String
    let blurb: String
}

struct Toolchain: Decodable, Identifiable {
    var id: String { language }
    let language: String
    let label: String
    let available: Bool
    let tool: String
    let detail: String
}

struct DetectResult: Decodable {
    let ok: Bool
    let language: String?
    let label: String?
    let reason: String?
    let entry: String?
    let projectRoot: String?
    let error: String?

    enum CodingKeys: String, CodingKey {
        case ok, language, label, reason, entry, error
        case projectRoot = "project_root"
    }
}

struct BuildEvent: Decodable {
    let ok: Bool
    let language: String?
    let label: String?
    let artifacts: [String]?
    let tool: String?
    let notes: [String]?
    let error: String?
}

struct StatusBanner: Identifiable {
    let id = UUID()
    let success: Bool
    let message: String
    let artifact: URL?
}

@MainActor
final class BuildSession: ObservableObject {
    @Published var sourcePath = ""
    @Published var outputPath = ""
    @Published var appName = ""
    @Published var language = "auto"
    @Published var mode = "app"
    @Published var iconPath = ""
    @Published var languages: [LanguageChoice] = []
    @Published var tools: [Toolchain] = []
    @Published var detection: DetectResult?
    @Published var detectError: String?
    @Published var logText = ""
    @Published var running = false
    @Published var loading = true
    @Published var engineMissing = false
    @Published var banner: StatusBanner?
    @Published var nameTouched = false

    private var environment: [String: String] = ProcessInfo.processInfo.environment
    private var detectTask: Task<Void, Never>?
    private var runningProcess: RunningProcess?
    private var pendingLog = ""
    private var suppressNameEdit = false
    private var cancelled = false

    init() {
        let defaults = UserDefaults.standard
        sourcePath = defaults.string(forKey: "sourcePath") ?? ""
        let desktop = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Desktop").path
        outputPath = defaults.string(forKey: "outputPath") ?? desktop
        appName = defaults.string(forKey: "appName") ?? ""
        language = defaults.string(forKey: "language") ?? "auto"
        mode = defaults.string(forKey: "mode") ?? "app"
        iconPath = defaults.string(forKey: "iconPath") ?? ""
        nameTouched = defaults.bool(forKey: "nameTouched")
    }

    var canBuild: Bool {
        !running
            && !engineMissing
            && !sourcePath.trimmingCharacters(in: .whitespaces).isEmpty
            && !outputPath.trimmingCharacters(in: .whitespaces).isEmpty
            && !appName.trimmingCharacters(in: .whitespaces).isEmpty
    }

    var activeToolchain: Toolchain? {
        let id = language == "auto" ? detection?.language : language
        return tools.first { $0.language == id }
    }

    func bootstrap() async {
        environment = await Task.detached(priority: .userInitiated) {
            EngineClient.environment()
        }.value
        engineMissing = EngineClient.binaryURL() == nil
        await refreshToolchains()
        loading = false
        scheduleDetect()
    }

    func refreshToolchains() async {
        guard !engineMissing else { return }
        do {
            let (status, text) = try await EngineClient.collect(
                arguments: ["languages"],
                environment: environment
            )
            if status == 0, let data = text.data(using: .utf8),
               let decoded = try? JSONDecoder().decode(LanguagesEnvelope.self, from: data) {
                languages = decoded.languages
            }
            let toolsResult = try await EngineClient.collect(
                arguments: ["toolchains"],
                environment: environment
            )
            if toolsResult.0 == 0, let data = toolsResult.1.data(using: .utf8),
               let decoded = try? JSONDecoder().decode(ToolsEnvelope.self, from: data) {
                tools = decoded.tools
            }
        } catch {
            detectError = error.localizedDescription
        }
    }

    func scheduleDetect() {
        detectTask?.cancel()
        let path = sourcePath.trimmingCharacters(in: .whitespaces)
        guard !path.isEmpty else {
            detection = nil
            detectError = nil
            return
        }
        let language = language
        detectTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 200_000_000)
            guard !Task.isCancelled else { return }
            await self?.detect(path: path, language: language)
        }
    }

    func noteNameEdit() {
        if suppressNameEdit {
            suppressNameEdit = false
            persist()
            return
        }
        nameTouched = true
        persist()
    }

    func persist() {
        let defaults = UserDefaults.standard
        defaults.set(sourcePath, forKey: "sourcePath")
        defaults.set(outputPath, forKey: "outputPath")
        defaults.set(appName, forKey: "appName")
        defaults.set(language, forKey: "language")
        defaults.set(mode, forKey: "mode")
        defaults.set(iconPath, forKey: "iconPath")
        defaults.set(nameTouched, forKey: "nameTouched")
    }

    func chooseSource() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.message = "Choose a source file or a project folder"
        panel.prompt = "Choose"
        if panel.runModal() == .OK, let url = panel.url {
            sourcePath = url.path
            nameTouched = false
            persist()
            scheduleDetect()
        }
    }

    func chooseOutput() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = true
        panel.prompt = "Choose"
        panel.message = "Choose where to put the built app"
        if panel.runModal() == .OK, let url = panel.url {
            outputPath = url.path
            persist()
        }
    }

    func chooseIcon() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowedContentTypes = [UTType(filenameExtension: "icns") ?? .data, .png, .jpeg]
        panel.prompt = "Choose"
        panel.message = "Choose a .icns or .png icon"
        if panel.runModal() == .OK, let url = panel.url {
            iconPath = url.path
            persist()
        }
    }

    func clearIcon() {
        iconPath = ""
        persist()
    }

    func start() {
        guard canBuild else { return }
        persist()
        cancelled = false
        running = true
        banner = nil
        logText = ""
        pendingLog = ""
        var arguments = [
            "build",
            "--source", sourcePath,
            "--output", outputPath,
            "--name", appName.trimmingCharacters(in: .whitespaces),
            "--mode", mode,
            "--language", language
        ]
        if !iconPath.isEmpty {
            arguments.append(contentsOf: ["--icon", iconPath])
        }
        let environment = environment
        let processBox = RunningProcess()
        runningProcess = processBox
        Task {
            do {
                let status = try await EngineClient.run(
                    processBox,
                    arguments: arguments,
                    environment: environment
                ) { [session = self] chunk in
                    Task { @MainActor in
                        session.appendChunk(chunk)
                    }
                }
                finishPending()
                running = false
                runningProcess = nil
                if cancelled {
                    banner = StatusBanner(success: false, message: "Build stopped", artifact: nil)
                    return
                }
                if let event = latestEvent {
                    if event.ok {
                        let artifact = event.artifacts?.first.map { URL(fileURLWithPath: $0) }
                        let label = event.label ?? "Project"
                        banner = StatusBanner(success: true, message: "Built \(label)", artifact: artifact)
                    } else {
                        banner = StatusBanner(
                            success: false,
                            message: event.error ?? "Build failed",
                            artifact: nil
                        )
                    }
                } else if status != 0 {
                    banner = StatusBanner(success: false, message: "Build failed", artifact: nil)
                }
            } catch {
                finishPending()
                running = false
                runningProcess = nil
                appendLine(error.localizedDescription)
                banner = StatusBanner(success: false, message: error.localizedDescription, artifact: nil)
            }
        }
    }

    func stop() {
        cancelled = true
        runningProcess?.stop()
    }

    func copyLog() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(logText, forType: .string)
    }

    func reveal(_ url: URL) {
        NSWorkspace.shared.activateFileViewerSelecting([url])
    }

    private var latestEvent: BuildEvent?

    private func detect(path: String, language: String) async {
        guard !engineMissing else { return }
        var arguments = ["detect", path]
        if language != "auto" {
            arguments.append(contentsOf: ["--language", language])
        }
        do {
            let (status, text) = try await EngineClient.collect(
                arguments: arguments,
                environment: environment
            )
            guard let data = text.data(using: .utf8),
                  let result = try? JSONDecoder().decode(DetectResult.self, from: data) else {
                detection = nil
                detectError = status == 0 ? nil : "Could not read the project."
                return
            }
            if result.ok {
                detection = result
                detectError = nil
                applyName(from: result)
            } else {
                detection = nil
                detectError = result.error
            }
        } catch {
            detectError = error.localizedDescription
        }
    }

    private func applyName(from result: DetectResult) {
        guard !nameTouched else { return }
        let entry = result.entry ?? sourcePath
        var name = URL(fileURLWithPath: entry).deletingPathExtension().lastPathComponent
        let markers = [
            "Cargo", "package", "Package", "go", "pom", "pubspec", "Makefile",
            "makefile", "CMakeLists", "build", "requirements", "setup", "tsconfig"
        ]
        if markers.contains(name) {
            name = URL(fileURLWithPath: result.projectRoot ?? sourcePath).lastPathComponent
        }
        guard !name.isEmpty, name != appName else { return }
        suppressNameEdit = true
        appName = name
        persist()
    }

    private func appendChunk(_ chunk: String) {
        pendingLog += chunk
        while let range = pendingLog.range(of: "\n") {
            let line = String(pendingLog[..<range.lowerBound])
            pendingLog.removeSubrange(..<range.upperBound)
            appendLine(line)
        }
    }

    private func finishPending() {
        if !pendingLog.isEmpty {
            appendLine(pendingLog)
            pendingLog = ""
        }
    }

    private func appendLine(_ line: String) {
        if line.hasPrefix("@@PACKAGER@@ ") {
            let payload = String(line.dropFirst("@@PACKAGER@@ ".count))
            if let data = payload.data(using: .utf8),
               let event = try? JSONDecoder().decode(BuildEvent.self, from: data) {
                latestEvent = event
                if let notes = event.notes {
                    for note in notes where !note.isEmpty {
                        logText += note + "\n"
                    }
                }
            }
            return
        }
        if logText.count > 400_000 {
            logText = String(logText.suffix(300_000))
        }
        logText += line + "\n"
    }
}

private struct LanguagesEnvelope: Decodable {
    let languages: [LanguageChoice]
}

private struct ToolsEnvelope: Decodable {
    let tools: [Toolchain]
}
