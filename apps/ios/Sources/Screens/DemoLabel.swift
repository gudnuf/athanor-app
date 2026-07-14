import SwiftUI

// Honest labeling for the scripted DemoEngine fallback (real-device trust fix:
// the operator held a whole session with the canned reel believing it was
// live). Shown whenever `model.engine.isReal == false` — that state is the SAME
// whether a keyless build silently fell back or a deliberate demo/public
// checkout has no key configured at all, so the wording stays neutral either
// way. Two pieces: a persistent marker that's always in view, and one calm
// one-time line above the first scripted reply.

/// Persistent in-palette marker that the current engine is the scripted demo.
/// Muted small caps — quiet, never an alert, but always present so nobody
/// converses with the reel unaware. Used where there's no header row to fold
/// the marker into (e.g. InitiationScreen); SessionScreen inlines the same
/// text into its register header instead.
struct ScriptedDemoBadge: View {
    var body: some View {
        Text("scripted demo")
            .font(Ember.F.sans(11, weight: .bold))
            .textCase(.uppercase)
            .tracking(1.4)
            .foregroundStyle(Ember.C.mutedDim)
    }
}

/// One calm, italic system line shown once above the FIRST scripted reply. Not
/// an alert, not a color scream — a quiet serif aside that names the demo
/// honestly. Neutral wording covers both the keyless-fallback and the
/// deliberate-demo build (there is no key in either case).
struct DemoNoticeLine: View {
    var body: some View {
        Text("The Mystagogue is not live — this is a scripted demonstration. (No key was present at build.)")
            .font(Ember.F.serif(14, italic: true))
            .foregroundStyle(Ember.C.mutedDim)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}
