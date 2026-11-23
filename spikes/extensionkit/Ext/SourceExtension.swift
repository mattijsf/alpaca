import ExtensionFoundation
import Foundation

@main
struct SourceExtension: AppExtension {
    @AppExtensionPoint.Bind
    var extensionPoint: AppExtensionPoint {
        AppExtensionPoint.Identifier(host: "nl.mattix.spike.host", name: "source")
    }

    var configuration: ConnectionHandler {
        ConnectionHandler(onConnection: { connection in
            signal(SIGPIPE, SIG_IGN)
            connection.exportedInterface = NSXPCInterface(with: SourceProtocol.self)
            connection.exportedObject = Source()
            connection.resume()
            return true
        })
    }
}

final class Source: NSObject, SourceProtocol {
    func apiVersion(reply: @escaping (Int) -> Void) { reply(1) }
    func call(_ request: Data, reply: @escaping (Data) -> Void) { reply(request) }

    func openAudio(mode: Int, amount: Int, reply: @escaping (FileHandle?) -> Void) {
        let pipe = Pipe()
        reply(pipe.fileHandleForReading)
        Thread.detachNewThread {
            let fd = pipe.fileHandleForWriting.fileDescriptor
            var chunk = [UInt8](repeating: 0x55, count: mode == 0 ? 65536 : chunkBytes)
            func writeAll() -> Bool {
                var done = 0
                while done < chunk.count {
                    let n = chunk.withUnsafeBytes { write(fd, $0.baseAddress! + done, chunk.count - done) }
                    if n <= 0 { return false }
                    done += n
                }
                return true
            }
            if mode == 0 {
                var left = amount
                while left > 0, writeAll() { left -= chunk.count }
            } else {
                let period: UInt64 = 1_000_000_000 * 1024 / 44100
                let chunks = amount * 44100 / 1024
                let start = nowNanos()
                for i in 0..<chunks {
                    let due = start + UInt64(i) * period
                    let now = nowNanos()
                    if due > now { usleep(UInt32((due - now) / 1000)) }
                    var stamp = nowNanos()
                    withUnsafeBytes(of: &stamp) { chunk.replaceSubrange(0..<8, with: $0) }
                    if !writeAll() { break }
                }
            }
            try? pipe.fileHandleForWriting.close()
            try? pipe.fileHandleForReading.close()
        }
    }
}
