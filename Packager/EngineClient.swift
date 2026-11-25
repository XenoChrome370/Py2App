import Foundation

enum EngineError: LocalizedError {
    case missing
    case failed(String)

    var errorDescription: String? {
        switch self {
        case .missing:
            return "The packaging engine is not inside this app. Build the Packager target in Xcode."
        case .failed(let message):
            return message
        }
    }
}

enum EngineClient {
    static func binaryURL() -> URL? {
        let fileManager = FileManager.default
        if let override = ProcessInfo.processInfo.environment["PACKAGER_BIN"] {
            let url = URL(fileURLWithPath: override)
            if fileManager.isExecutableFile(atPath: url.path) {
                return url
            }
        }
        var candidates: [URL] = []
        if let auxiliary = Bundle.main.url(forAuxiliaryExecutable: "packager-engine") {
            candidates.append(auxiliary)
        }
        if let executable = Bundle.main.executableURL {
            candidates.append(executable.deletingLastPathComponent().appendingPathComponent("packager-engine"))
        }
        candidates.append(Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/packager-engine"))
        if let resources = Bundle.main.resourceURL {
            candidates.append(resources.appendingPathComponent("packager-engine"))
        }
        return candidates.first { fileManager.isExecutableFile(atPath: $0.path) }
    }

    static func environment() -> [String: String] {
        var values = ProcessInfo.processInfo.environment
        if let path = loginPath() {
            values["PATH"] = path
        }
        return values
    }

    static func collect(
        arguments: [String],
        environment: [String: String]
    ) async throws -> (Int32, String) {
        let buffer = TextBuffer()
        let status = try await run(
            RunningProcess(),
            arguments: arguments,
            environment: environment
        ) { chunk in
            buffer.append(chunk)
        }
        return (status, buffer.text)
    }

    static func run(
        _ running: RunningProcess,
        arguments: [String],
        environment: [String: String],
        onText: @escaping @Sendable (String) -> Void
    ) async throws -> Int32 {
        guard let binary = binaryURL() else {
            throw EngineError.missing
        }
        let process = running.process
        let pipe = Pipe()
        process.executableURL = binary
        process.arguments = arguments
        process.environment = environment
        process.standardOutput = pipe
        process.standardError = pipe
        process.standardInput = FileHandle.nullDevice
        if let home = environment["HOME"] {
            process.currentDirectoryURL = URL(fileURLWithPath: home, isDirectory: true)
        }
        do {
            try process.run()
        } catch {
            throw EngineError.failed(error.localizedDescription)
        }
        let handle = pipe.fileHandleForReading
        return await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                while true {
                    let data = handle.availableData
                    if !data.isEmpty, let text = String(data: data, encoding: .utf8) {
                        onText(text)
                    }
                    if !process.isRunning {
                        let rest = handle.availableData
                        if !rest.isEmpty, let text = String(data: rest, encoding: .utf8) {
                            onText(text)
                        }
                        break
                    }
                    if data.isEmpty {
                        Thread.sleep(forTimeInterval: 0.02)
                    }
                }
                process.waitUntilExit()
                continuation.resume(returning: process.terminationStatus)
            }
        }
    }

    private static func loginPath() -> String? {
        let shell = ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/zsh"
        guard FileManager.default.isExecutableFile(atPath: shell) else {
            return nil
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: shell)
        process.arguments = ["-lc", "printf %s \"$PATH\""]
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = Pipe()
        do {
            try process.run()
        } catch {
            return nil
        }
        let deadline = Date().addingTimeInterval(3)
        while process.isRunning && Date() < deadline {
            Thread.sleep(forTimeInterval: 0.05)
        }
        if process.isRunning {
            process.terminate()
            process.waitUntilExit()
            return nil
        }
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        let value = String(data: data, encoding: .utf8)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard let value, value.contains("/") else {
            return nil
        }
        return value
    }
}

final class RunningProcess: @unchecked Sendable {
    let process = Process()

    func stop() {
        if process.isRunning {
            process.terminate()
        }
    }
}

private final class TextBuffer: @unchecked Sendable {
    private let lock = NSLock()
    private var storage = ""

    func append(_ text: String) {
        lock.lock()
        storage += text
        lock.unlock()
    }

    var text: String {
        lock.lock()
        defer { lock.unlock() }
        return storage
    }
}
