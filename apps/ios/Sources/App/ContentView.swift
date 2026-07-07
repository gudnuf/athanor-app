import SwiftUI

/// Phase 0 placeholder. The shell owns no logic (rmp invariant, see
/// docs/invariants.md) — it renders state and dispatches actions. There is no
/// state or action yet: athanor-core is a stub until the spike gates land
/// (see docs/plans/). This screen exists so the workspace has a real
/// SwiftUI target to build, not to demonstrate any product surface.
struct ContentView: View {
    var body: some View {
        VStack(spacing: 12) {
            Text("Athanor")
                .font(.title)
            Text("the furnace is not yet lit")
                .foregroundStyle(.secondary)
        }
        .padding()
    }
}

#Preview {
    ContentView()
}
