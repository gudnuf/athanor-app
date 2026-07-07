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

    private static let initiationKey = "athanor.hasCompletedInitiation"

    init(engine: any AthanorEngineProtocol) {
        self.engine = engine
        self.hasCompletedInitiation = UserDefaults.standard.bool(forKey: Self.initiationKey)
    }
}
