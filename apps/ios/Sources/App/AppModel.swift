import Foundation
import Observation

// Owns navigation state + the engine handle. No business logic lives here —
// it dispatches to `AthanorEngineProtocol` and reflects what comes back
// (rmp invariant: no business logic in Swift).

@MainActor
@Observable
final class AppModel {
    let engine: any AthanorEngineProtocol

    /// First-launch routes to Initiation (spec: "First launch is the
    /// initiation"). Persisted locally so it only fires once per install.
    var hasCompletedInitiation: Bool {
        didSet { UserDefaults.standard.set(hasCompletedInitiation, forKey: Self.initiationKey) }
    }

    /// Which engine the Bellows uses. Apple on-device speech by default;
    /// whisper (base/small) is selectable from the dev overlay. Persisted,
    /// except when the QA launch arg `bellows-engine=<rawValue>` overrides it
    /// for this process. DemoEngine never looks at this.
    var bellowsEngine: BellowsEngine {
        didSet {
            guard !Self.engineOverridden else { return }
            UserDefaults.standard.set(bellowsEngine.rawValue, forKey: Self.bellowsEngineKey)
        }
    }

    /// Whisper model provisioning (F1). Lazy: nothing downloads unless a
    /// whisper engine is chosen (at launch or from the overlay). A first
    /// launch on Apple needs no download. The overlay and the whisper
    /// factory path read `modelDownloader.state`.
    let modelDownloader = ModelDownloader()

    private static let initiationKey = "athanor.hasCompletedInitiation"
    private static let bellowsEngineKey = "athanor.bellowsEngine"
    private static let engineArgPrefix = "bellows-engine="
    private static var engineOverridden: Bool {
        ProcessInfo.processInfo.arguments.contains { $0.hasPrefix(engineArgPrefix) }
    }

    init(engine: any AthanorEngineProtocol) {
        self.engine = engine
        self.hasCompletedInitiation = UserDefaults.standard.bool(forKey: Self.initiationKey)
        let override = ProcessInfo.processInfo.arguments
            .first { $0.hasPrefix(Self.engineArgPrefix) }
            .flatMap { BellowsEngine(rawValue: String($0.dropFirst(Self.engineArgPrefix.count))) }
        let stored = UserDefaults.standard.string(forKey: Self.bellowsEngineKey).flatMap(BellowsEngine.init(rawValue:))
        self.bellowsEngine = override ?? stored ?? .apple
        if let tier = self.bellowsEngine.whisperTier {
            Task { [modelDownloader] in
                await modelDownloader.ensureModel(tier: tier)
            }
        }
    }
}
