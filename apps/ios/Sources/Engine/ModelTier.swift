import Foundation

// Whisper model tiers for the Bellows' on-device STT (plan §2, Task F1).
// Base ships as the default; small is an opt-in config knob the Bellows path
// reads later (C3) — nothing here talks to `crates/stt` or any FFI type.
enum ModelTier: String, CaseIterable, Identifiable, Codable {
    case base
    case small

    var id: String { rawValue }

    /// Matches the exact filenames the ggerganov/whisper.cpp HF mirror
    /// publishes (same repo the whisper spike already used).
    var fileName: String {
        switch self {
        case .base: return "ggml-base.en-q5_1.bin"
        case .small: return "ggml-small.en-q5_1.bin"
        }
    }

    var remoteURL: URL {
        URL(string: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/\(fileName)")!
    }

    /// Approximate size (plan §2), used only to seed a progress fraction
    /// before the real Content-Length is known — never trusted for
    /// verification (that's Content-Length + SHA-256, see ModelDownloader).
    var approxByteCount: Int64 {
        switch self {
        case .base: return 57_000_000
        case .small: return 182_000_000
        }
    }

    var displayName: String {
        switch self {
        case .base: return "Base"
        case .small: return "Small"
        }
    }
}
