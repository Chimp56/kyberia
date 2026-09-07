// Test-only blocked-work process using the production lifecycle and wire writer.
// No Wi-Fi API, environment spoofing, or synthetic production collector path.
import Darwin
import Foundation

@main
struct ProcessHarness {
    static func main() {
        let writer = WireWriter()
        let control = ProcessControl(writer: writer, timeoutSeconds: 1)
        withExtendedLifetime(control) {
            writer.emit("hello", ["collector": "kyberia-macos-corewlan", "collector_version": collectorVersion,
                                  "collector_build": "sha256:" + String(repeating: "1", count: 64),
                                  "evidence_origin": "synthetic_fixture", "os_version": "test-only-process",
                                  "command": "scan", "identifier_policy": "redacted", "max_observations": 1,
                                  "max_record_bytes": 16384, "timeout_seconds": 1, "clock_epoch": writer.session])
            if CommandLine.arguments.contains("--runloop-callback") {
                _ = Timer.scheduledTimer(withTimeInterval: 0.02, repeats: false) { _ in
                    writer.finish("error", reason: "test_runloop_callback_delivered", exitCode: 70)
                }
            }
            runCollectorLoop()
        }
    }
}
