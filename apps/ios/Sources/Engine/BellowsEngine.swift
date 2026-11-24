import Foundation

// Which engine turns the learner's voice into text (Bellows on Apple Speech
// plan, Task 6). Apple's on-device recognizer is the default; the whisper
// tiers stay selectable from the dev overlay (triple-tap the mask name).
enum BellowsEngine: String, CaseIterable, Identifiable, Codable {
    case apple, whisperBase = "whisper-base", whisperSmall = "whisper-small"
    var id: String { rawValue }
    var whisperTier: ModelTier? { switch self { case .apple: nil; case .whisperBase: .base; case .whisperSmall: .small } }
    var displayName: String { switch self { case .apple: "Apple"; case .whisperBase: "W·Base"; case .whisperSmall: "W·Small" } }
}
