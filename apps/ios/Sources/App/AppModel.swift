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

    /// Whisper model tier (Task F1) — base default, small opt-in. Read by
    /// the real Bellows path (post-C3/D2) when opening `BellowsHandle`;
    /// DemoEngine never looks at this.
    var modelTier: ModelTier {
        didSet { UserDefaults.standard.set(modelTier.rawValue, forKey: Self.modelTierKey) }
    }

    /// First-launch model provisioning (F1). Starts downloading immediately
    /// at app launch (idempotent — a no-op if already verified on disk);
    /// InitiationScreen's warming line and E4's real-Bellows gate both read
    /// `modelDownloader.state`.
    let modelDownloader = ModelDownloader()

    private static let initiationKey = "athanor.hasCompletedInitiation"
    private static let modelTierKey = "athanor.modelTier"

    init(engine: any AthanorEngineProtocol) {
        self.engine = engine
        self.hasCompletedInitiation = UserDefaults.standard.bool(forKey: Self.initiationKey)
        let storedTier = UserDefaults.standard.string(forKey: Self.modelTierKey).flatMap(ModelTier.init(rawValue:))
        self.modelTier = storedTier ?? .base
        let tier = self.modelTier
        Task { [modelDownloader] in
            await modelDownloader.ensureModel(tier: tier)
        }
    }
}
