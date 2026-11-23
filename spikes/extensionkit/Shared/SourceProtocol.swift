import Foundation

/// The whole contract of the spike: a version, one opaque request/answer, and the audio as a pipe.
@objc protocol SourceProtocol {
    func apiVersion(reply: @escaping (Int) -> Void)
    func call(_ request: Data, reply: @escaping (Data) -> Void)
    /// mode 0: `amount` bytes as fast as the pipe takes them. mode 1: `amount` seconds of 16-bit
    /// stereo 44.1 kHz at real time, 1024 frames a chunk, each chunk starting with the time it was written.
    func openAudio(mode: Int, amount: Int, reply: @escaping (FileHandle?) -> Void)
}

let chunkBytes = 4096
func nowNanos() -> UInt64 { clock_gettime_nsec_np(CLOCK_MONOTONIC_RAW) }
