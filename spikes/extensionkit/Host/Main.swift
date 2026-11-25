import AppKit
import ExtensionFoundation
import ExtensionKit

extension AppExtensionPoint {
    @Definition
    static var source: AppExtensionPoint {
        Name("source")
        UserInterface(false)
        Scope(restriction: .none)
    }
}

func log(_ line: String) { print(line); fflush(stdout) }
func ms(_ nanos: UInt64) -> String { String(format: "%.3f ms", Double(nanos) / 1e6) }

@main
struct Main {
    static func main() {
        let app = NSApplication.shared
        if CommandLine.arguments.contains("--browse") {
            // the system's own list of this app's extensions, where the listener switches one on
            app.setActivationPolicy(.regular)
            let window = NSWindow(contentViewController: EXAppExtensionBrowserViewController())
            window.title = "Sources"
            window.setContentSize(NSSize(width: 520, height: 420))
            window.center()
            window.makeKeyAndOrderFront(nil)
            app.activate(ignoringOtherApps: true)
            NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: window, queue: .main) { _ in exit(0) }
            app.run()
            return
        }
        app.setActivationPolicy(.accessory)
        Task {
            await run()
            exit(0)
        }
        app.run()
    }
}

func run() async {
    do {
        log("extension point id: \(AppExtensionPoint.source.id)")
        let monitor = try await AppExtensionPoint.Monitor(appExtensionPoint: .source)
        try await Task.sleep(for: .milliseconds(800))
        let state = monitor.state
        log("identities \(state.identities.count), disabled \(state.disabledCount), unapproved \(state.unapprovedCount)")
        for identity in state.identities {
            log("== \(identity.bundleIdentifier) \"\(identity.localizedName)\" point \(identity.extensionPointIdentifier)")
            do { try await exercise(identity) } catch { log("   failed: \(error)") }
        }
    } catch {
        log("error: \(error)")
    }
}

func exercise(_ identity: AppExtensionIdentity) async throws {
    let t0 = nowNanos()
    let process = try await AppExtensionProcess(configuration: .init(appExtensionIdentity: identity, onInterruption: { log("   interrupted") }))
    let connection = try process.makeXPCConnection()
    connection.remoteObjectInterface = NSXPCInterface(with: SourceProtocol.self)
    connection.resume()
    guard let source = connection.remoteObjectProxyWithErrorHandler({ log("   xpc error: \($0)") }) as? SourceProtocol else {
        log("   no proxy"); return
    }
    let version: Int = await withCheckedContinuation { c in source.apiVersion { c.resume(returning: $0) } }
    log("   launch + first answer: \(ms(nowNanos() - t0)), apiVersion \(version)")

    // 2000 request/answer round trips of 256 bytes
    let request = Data(repeating: 7, count: 256)
    var times: [UInt64] = []
    for _ in 0..<2000 {
        let t = nowNanos()
        let _: Data = await withCheckedContinuation { c in source.call(request) { c.resume(returning: $0) } }
        times.append(nowNanos() - t)
    }
    times.sort()
    log("   call round trip: p50 \(ms(times[1000])), p99 \(ms(times[1980])), max \(ms(times[1999]))")

    // the pipe as fast as it goes
    let burst = 512 * 1024 * 1024
    if let handle: FileHandle = await withCheckedContinuation({ c in source.openAudio(mode: 0, amount: burst) { c.resume(returning: $0) } }) {
        let (bytes, nanos): (Int, UInt64) = await withCheckedContinuation { c in
            Thread.detachNewThread {
                let fd = handle.fileDescriptor
                var buffer = [UInt8](repeating: 0, count: 65536)
                var total = 0
                let t = nowNanos()
                while true {
                    let n = buffer.withUnsafeMutableBytes { read(fd, $0.baseAddress!, 65536) }
                    if n <= 0 { break }
                    total += n
                }
                c.resume(returning: (total, nowNanos() - t))
            }
        }
        let rate = Double(bytes) / 1e6 / (Double(nanos) / 1e9)
        log(String(format: "   pipe burst: %d MB in %.2f s = %.0f MB/s (%.0fx what 44.1 kHz 16-bit stereo needs)", bytes >> 20, Double(nanos) / 1e9, rate, rate / 0.1764))
    } else { log("   no pipe") }

    // the pipe at real time: how late each chunk of 1024 frames arrives
    let seconds = 10
    if let handle: FileHandle = await withCheckedContinuation({ c in source.openAudio(mode: 1, amount: seconds) { c.resume(returning: $0) } }) {
        let late: [UInt64] = await withCheckedContinuation { c in
            Thread.detachNewThread {
                let fd = handle.fileDescriptor
                var buffer = [UInt8](repeating: 0, count: chunkBytes)
                var late: [UInt64] = []
                outer: while true {
                    var done = 0
                    while done < chunkBytes {
                        let n = buffer.withUnsafeMutableBytes { read(fd, $0.baseAddress! + done, chunkBytes - done) }
                        if n <= 0 { break outer }
                        done += n
                    }
                    let stamp = buffer.withUnsafeBytes { $0.loadUnaligned(as: UInt64.self) }
                    late.append(nowNanos() &- stamp)
                }
                c.resume(returning: late)
            }
        }
        let sorted = late.sorted()
        if sorted.isEmpty { log("   paced: nothing arrived") } else {
            log("   pipe at real time, \(sorted.count) chunks over \(seconds) s: write-to-read p50 \(ms(sorted[sorted.count / 2])), p99 \(ms(sorted[sorted.count * 99 / 100])), max \(ms(sorted.last!))")
        }
    }
    process.invalidate()
}
