import SwiftUI

// The reading view for a past fire: the full, role-tagged transcript of one
// closed session — nothing lost. The learner's own words sit muted and to the
// right (as in a live session); the Mystagogue speaks in the serif reading
// register. Above the dialogue, the session's residue — the condensation note
// it distilled on close — is set down in gold, the settled sediment of the work.
//
// Reached from a thread's detail (ThreadDetailScreen) or the "past fires" list.
struct TranscriptView: View {
    var model: AppModel
    let sessionId: String

    @State private var detail: SessionDetail?

    var body: some View {
        Group {
            if let detail {
                content(detail)
            } else {
                // The session couldn't be read (unknown id) — a calm, in-palette
                // line rather than a blank void.
                VStack(spacing: 10) {
                    Text(Ember.Glyph.furnace)
                        .font(.system(size: 26))
                        .foregroundStyle(Ember.C.mutedDim)
                    Text("This fire left no record.")
                        .font(Ember.F.serif(16, italic: true))
                        .foregroundStyle(Ember.C.muted)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(Ember.C.ground.ignoresSafeArea())
        .navigationTitle(title)
        .navigationBarTitleDisplayMode(.inline)
        .task { detail = model.engine.sessionDetail(sessionId) }
    }

    private var title: String {
        guard let detail else { return "a past fire" }
        return "\(detail.mask) · \(Self.dateLabel(detail.date))"
    }

    private func content(_ detail: SessionDetail) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                header(detail)
                if let note = detail.note, !note.isEmpty {
                    ResidueCard(note: note)
                }
                ForEach(detail.turns) { turn in
                    turnView(turn)
                }
            }
            .padding(.horizontal, Ember.S.screenPad)
            .padding(.vertical, 20)
        }
    }

    private func header(_ detail: SessionDetail) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 8) {
                Text(Ember.Glyph.fireMask).foregroundStyle(Ember.C.heat)
                Text(detail.mask)
                    .font(Ember.F.sans(12, weight: .bold))
                    .tracking(1.2)
                    .textCase(.uppercase)
                    .foregroundStyle(Ember.C.ink)
                Text("·").foregroundStyle(Ember.C.mutedDim)
                Text(detail.mode)
                    .font(Ember.F.sans(12))
                    .textCase(.uppercase)
                    .tracking(1.0)
                    .foregroundStyle(Ember.C.muted)
            }
            Text(Self.dateLabel(detail.date))
                .font(Ember.F.sans(11.5))
                .foregroundStyle(Ember.C.mutedDim)
                .monospacedDigit()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.bottom, 2)
        .overlay(alignment: .bottom) { Ember.C.hairline.frame(height: 1) }
    }

    @ViewBuilder
    private func turnView(_ turn: TranscriptTurn) -> some View {
        switch turn.role {
        case .mystagogue:
            // The Mystagogue's voice in the serif reading register (formatted
            // light markdown, settled — this is history, never mid-stream).
            StreamingText(text: turn.text, register: .serif, formatted: true)
        case .learner:
            // The learner's own words — muted, to the right, as in a session.
            Text(turn.text)
                .font(Ember.F.sans(15))
                .foregroundStyle(Ember.C.muted)
                .frame(maxWidth: 280, alignment: .trailing)
                .frame(maxWidth: .infinity, alignment: .trailing)
        }
    }

    static func dateLabel(_ date: Date) -> String {
        let f = DateFormatter()
        f.dateFormat = "MMM d, yyyy"
        return f.string(from: date)
    }
}

/// The session's residue — the condensation note it distilled on close. Gold,
/// once, still: the sediment the fire left. (Gold is reserved for salt/
/// condensation moments; a session note IS the residue of condensation.)
private struct ResidueCard: View {
    var note: String

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Text(Ember.Glyph.grimoire).foregroundStyle(Ember.C.gold)
                Text("residue")
                    .font(Ember.F.sans(11, weight: .bold))
                    .tracking(1.2)
                    .textCase(.uppercase)
                    .foregroundStyle(Ember.C.gold)
            }
            Text(note)
                .font(Ember.F.serif(15.5))
                .foregroundStyle(Ember.C.ink)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Ember.C.raised, in: RoundedRectangle(cornerRadius: 14))
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(Ember.C.gold.opacity(0.28), lineWidth: 1))
    }
}
