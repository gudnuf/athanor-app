import SwiftUI

// Renders the Mystagogue's voice as it streams in, word by word, without
// ever visibly jumping. Two things make that true:
//
// 1. `.animation(nil, value: text)` — streaming text is explicitly OUTSIDE
//    the motion budget (Ember.Motion). The three named moments (furnace
//    fire, bellows embers, condensation) are the only things allowed to
//    animate; text growing as deltas arrive must be instant, or the
//    "restraint is part of the spec" rule is broken by the busiest thing on
//    the session screen.
// 2. Fixed leading alignment + `.fixedSize(vertical:)` — new words extend the
//    text forward/downward only; already-rendered words never re-lay-out
//    because of what arrives after them.
//
// Both DemoEngine (today) and the real engine (post-D2) hand this view
// accumulated text one delta at a time; this view doesn't know or care which
// one is behind it.
struct StreamingText: View {
    let text: String
    let register: ReplyRegister

    var body: some View {
        Text(text)
            .font(font)
            .foregroundStyle(Ember.C.ink)
            .multilineTextAlignment(.leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .fixedSize(horizontal: false, vertical: true)
            .animation(Ember.Motion.none, value: text)
    }

    private var font: Font {
        switch register {
        case .quick: return Ember.F.sans(15, weight: .medium)
        case .serif: return Ember.F.serif(19)
        }
    }
}
