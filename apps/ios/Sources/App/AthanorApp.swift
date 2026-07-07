import SwiftUI

// Entry point + engine selection.
//
// E1 demo path is FFI-free (plan review edit #3): there is no
// `#if canImport(AthanorCoreFFI)` here yet, unlike murmur-rmp's GalleryApp —
// that seam lands with D2/D3 when the real xcframework exists. Today this
// always resolves `DemoEngine`. The Keychain key is still read at startup
// (per E1 scope) so the real-engine wiring later is a pure addition: swap the
// `DemoEngine()` below for `AthanorCoreEngine(apiKey:...)` when a key is
// present and the FFI package is linked — nothing above this function needs
// to change shape.
@MainActor
private func resolveEngine() -> any AthanorEngineProtocol {
    // Read (not used yet): DemoEngine ignores it entirely; this only proves
    // the Keychain round-trip works before the real engine needs it.
    _ = KeychainKeyStore.load()
    return DemoEngine()
}

@main
struct AthanorApp: App {
    @State private var model: AppModel

    init() {
        _model = State(initialValue: AppModel(engine: resolveEngine()))
    }

    var body: some Scene {
        WindowGroup {
            RootRouter(model: model)
        }
    }
}

/// Routes first launch to Initiation; thereafter to the Furnace shell.
struct RootRouter: View {
    var model: AppModel

    var body: some View {
        Group {
            if model.hasCompletedInitiation {
                FurnaceShell(model: model)
            } else {
                InitiationScreen(model: model)
            }
        }
        .preferredColorScheme(.dark)
        .tint(Ember.C.heat)
    }
}
