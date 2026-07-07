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
//
// Register also carries visual shape (mockups-v2.html `.say-teacher` /
// `.say-quick`): `serif` is the full-width reading voice (a lesson);
// `quick` is a left-bordered, heat-tinted note (a conversational aside).
// The register switch itself is instant — it's a style choice per message,
// not a motion-budget item.
//
// MARKDOWN (the Mystagogue emits light markdown — bold/italic/code): rendered
// only once a reply has SETTLED (`formatted: true`), never mid-stream. This is
// the deliberate choice to protect the no-reflow contract: interpreting a
// `**bold**` span REMOVES its markers, which shrinks already-laid-out words —
// exactly the re-layout point (2) forbids. So the in-flight text streams raw
// (markers visible, growing forward only), and formats in one calm step the
// instant `turnComplete` moves it into the settled transcript. The learner's
// own transcribed words are always rendered plain (never `formatted`).
struct StreamingText: View {
    let text: String
    let register: ReplyRegister
    /// True once the reply has settled — render inline markdown. False (default)
    /// while streaming — render raw so no marker resolution re-lays-out words.
    var formatted: Bool = false

    private var rendered: AttributedString {
        formatted ? .mystagogueInline(text) : AttributedString(text)
    }

    var body: some View {
        Group {
            switch register {
            case .serif:
                Text(rendered)
                    .font(Ember.F.serif(19))
                    .foregroundStyle(Ember.C.ink)
                    .frame(maxWidth: .infinity, alignment: .leading)
            case .quick:
                Text(rendered)
                    .font(Ember.F.sans(15, weight: .medium))
                    .foregroundStyle(Ember.C.heatHot)
                    .padding(.vertical, 9)
                    .padding(.horizontal, 14)
                    .background(Ember.C.heat.opacity(0.07))
                    .overlay(alignment: .leading) {
                        Rectangle().fill(Ember.C.heat.opacity(0.5)).frame(width: 2)
                    }
                    .clipShape(
                        UnevenRoundedRectangle(topLeadingRadius: 0, bottomLeadingRadius: 0, bottomTrailingRadius: 10, topTrailingRadius: 10)
                    )
                    .frame(maxWidth: 280, alignment: .leading)
            }
        }
        .multilineTextAlignment(.leading)
        .fixedSize(horizontal: false, vertical: true)
        .animation(Ember.Motion.none, value: text)
    }
}

extension AttributedString {
    /// The Mystagogue's light markdown → an `AttributedString`: inline only
    /// (bold / italic / code), whitespace and newlines PRESERVED so the reply's
    /// paragraph breaks survive. Falls back to the raw text if parsing ever
    /// fails — a reply must never vanish because a marker was malformed.
    static func mystagogueInline(_ s: String) -> AttributedString {
        let options = AttributedString.MarkdownParsingOptions(
            interpretedSyntax: .inlineOnlyPreservingWhitespace,
            failurePolicy: .returnPartiallyParsedIfPossible
        )
        return (try? AttributedString(markdown: s, options: options)) ?? AttributedString(s)
    }
}
