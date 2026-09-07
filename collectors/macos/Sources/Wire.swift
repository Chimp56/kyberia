import Darwin
import Foundation

let protocolVersion = "kyberia.macos.collector/1"
let collectorVersion = "0.1.0"

/// CoreLocation delivers delegate callbacks on the initialization run loop.
/// Keep it alive even when the only outstanding operation uses a dispatch queue.
func runCollectorLoop() -> Never {
    precondition(Thread.isMainThread)
    let keepAlive = Timer(timeInterval: 3600, repeats: true) { _ in }
    RunLoop.main.add(keepAlive, forMode: .default)
    RunLoop.main.run()
    diagnostic("collector main run loop unexpectedly stopped")
    _exit(70)
}

func unknown(_ reason: String) -> [String: Any] {
    ["state": "unknown", "reason": reason]
}

func known(_ value: Any) -> [String: Any] {
    ["state": "known", "value": value]
}

func timestamp() -> [String: Any] {
    let format = ISO8601DateFormatter()
    format.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return ["receipt_utc": format.string(from: Date()),
            "receipt_monotonic_ns": String(DispatchTime.now().uptimeNanoseconds),
            "capture_time": unknown("source_did_not_provide"),
            "clock_uncertainty_seconds": unknown("not_calibrated")]
}

/// One serialization point for bounded records and the terminal state.
/// Nonblocking stdout prevents a stalled consumer holding the worker forever.
final class WireWriter: @unchecked Sendable {
    let session = UUID().uuidString.lowercased()
    private let lock = NSLock()
    private var sequence = 0
    private var ended = false
    private var observations = 0

    init() {
        signal(SIGPIPE, SIG_IGN)
        _ = fcntl(STDOUT_FILENO, F_SETFL, fcntl(STDOUT_FILENO, F_GETFL) | O_NONBLOCK)
        _ = fcntl(STDERR_FILENO, F_SETFL, fcntl(STDERR_FILENO, F_GETFL) | O_NONBLOCK)
    }

    private func writeLocked(_ kind: String, _ body: [String: Any]) {
        var record = body
        record["protocol"] = protocolVersion
        record["sequence"] = sequence
        record["session_id"] = session
        record["kind"] = kind
        record["time"] = timestamp()
        guard JSONSerialization.isValidJSONObject(record),
              let data = try? JSONSerialization.data(withJSONObject: record, options: [.sortedKeys]),
              data.count < 16 * 1024 else {
            diagnostic("invalid or oversized output record")
            _exit(74)
        }
        var bytes = Array(data)
        bytes.append(10)
        let deadline = DispatchTime.now().uptimeNanoseconds + 500_000_000
        bytes.withUnsafeBytes { buffer in
            var offset = 0
            while offset < buffer.count {
                let count = Darwin.write(STDOUT_FILENO, buffer.baseAddress!.advanced(by: offset), buffer.count - offset)
                if count > 0 {
                    offset += count
                } else if count < 0 && errno == EINTR {
                    continue
                } else if count < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)
                            && DispatchTime.now().uptimeNanoseconds < deadline {
                    usleep(1000)
                } else {
                    diagnostic("output consumer unavailable; incomplete stream")
                    _exit(74)
                }
            }
        }
        sequence += 1
    }

    func emit(_ kind: String, _ body: [String: Any]) {
        lock.lock()
        defer { lock.unlock() }
        guard !ended else { return }
        writeLocked(kind, body)
        if kind == "scan_observation" { observations += 1 }
    }

    func finish(_ status: String, reason: String, exitCode: Int32, extra: [String: Any] = [:]) -> Never {
        lock.lock()
        if !ended {
            ended = true
            var body = extra
            body["status"] = status
            body["reason"] = reason
            body["observation_count"] = observations
            body["partial"] = status != "ok" && observations > 0
            writeLocked("complete", body)
        }
        lock.unlock()
        exit(exitCode)
    }
}

func diagnostic(_ message: String) {
    let bytes = Array(("kyberia-macos: " + message + "\n").utf8)
    bytes.withUnsafeBytes { buffer in
        _ = Darwin.write(STDERR_FILENO, buffer.baseAddress, buffer.count)
    }
}

struct Options {
    let command: String
    var timeoutSeconds = 20
    var limit = 256
    var interfaceName: String?
    var includeIdentifiers = false

    init(_ arguments: [String]) throws {
        guard let command = arguments.first, ["probe", "scan", "authorize"].contains(command) else {
            throw ConfigurationError.invalid
        }
        self.command = command
        var index = 1
        while index < arguments.count {
            let argument = arguments[index]
            if argument == "--include-identifiers" {
                includeIdentifiers = true
            } else {
                index += 1
                guard index < arguments.count else { throw ConfigurationError.invalid }
                let value = arguments[index]
                switch argument {
                case "--timeout-seconds":
                    guard let n = Int(value), (1...60).contains(n) else { throw ConfigurationError.invalid }
                    timeoutSeconds = n
                case "--limit":
                    guard let n = Int(value), (1...4096).contains(n) else { throw ConfigurationError.invalid }
                    limit = n
                case "--interface":
                    guard !value.isEmpty, value.utf8.count <= 64,
                          value.unicodeScalars.allSatisfy({ $0.isASCII && (CharacterSet.alphanumerics.contains($0) || $0 == "_") }) else {
                        throw ConfigurationError.invalid
                    }
                    interfaceName = value
                default: throw ConfigurationError.invalid
                }
            }
            index += 1
        }
    }
}

enum ConfigurationError: Error { case invalid }

/// Process-level cancellation is independent of CoreWLAN's blocking scan API.
/// Closing the process cancels our work; the OS may finish its own scan internally.
final class ProcessControl {
    private let timeout: DispatchSourceTimer
    private let interrupt: DispatchSourceSignal
    private let terminate: DispatchSourceSignal

    init(writer: WireWriter, timeoutSeconds: Int) {
        signal(SIGINT, SIG_IGN)
        signal(SIGTERM, SIG_IGN)
        timeout = DispatchSource.makeTimerSource(queue: DispatchQueue.global())
        timeout.schedule(deadline: .now() + .seconds(timeoutSeconds))
        timeout.setEventHandler {
            writer.finish("timeout", reason: "process_deadline_exceeded", exitCode: 124)
        }
        interrupt = DispatchSource.makeSignalSource(signal: SIGINT, queue: DispatchQueue.global())
        interrupt.setEventHandler { writer.finish("cancelled", reason: "sigint", exitCode: 130) }
        terminate = DispatchSource.makeSignalSource(signal: SIGTERM, queue: DispatchQueue.global())
        terminate.setEventHandler { writer.finish("cancelled", reason: "sigterm", exitCode: 143) }
        timeout.resume()
        interrupt.resume()
        terminate.resume()
    }
}
