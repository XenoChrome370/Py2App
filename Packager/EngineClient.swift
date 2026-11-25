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
                var decoder = Utf8Decoder()
                while true {
                    let data = handle.availableData
                    if !data.isEmpty {
                        let text = decoder.append(data)
                        if !text.isEmpty {
                            onText(text)
                        }
                    }
                    if !process.isRunning {
                        let rest = handle.availableData
                        if !rest.isEmpty {
                            let text = decoder.append(rest)
                            if !text.isEmpty {
                                onText(text)
                            }
                        }
                        let tail = decoder.finish()
                        if !tail.isEmpty {
                            onText(tail)
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

/// Keeps a short tail when a read splits a UTF-8 character across chunks.
private struct Utf8Decoder {
    private var pending = Data()

    mutating func append(_ data: Data) -> String {
        guard !data.isEmpty else { return "" }
        pending.append(data)
        return drain(flush: false)
    }

    mutating func finish() -> String {
        drain(flush: true)
    }

    private mutating func drain(flush: Bool) -> String {
        guard !pending.isEmpty else { return "" }
        if let text = String(data: pending, encoding: .utf8) {
            pending.removeAll(keepingCapacity: true)
            return text
        }
        let limit = min(3, pending.count - 1)
        if limit > 0 {
            for hold in 1...limit {
                let split = pending.count - hold
                if let text = String(data: pending.prefix(split), encoding: .utf8) {
                    pending.removeFirst(split)
                    return text
                }
            }
        }
        if flush || pending.count > 3 {
            let keep = flush ? 0 : 3
            let split = pending.count - keep
            let text = String(decoding: pending.prefix(split), as: UTF8.self)
            if keep == 0 {
                pending.removeAll(keepingCapacity: true)
            } else {
                pending.removeFirst(split)
            }
            return text
        }
        return ""
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
