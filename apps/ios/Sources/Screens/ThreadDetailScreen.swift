import SwiftUI

// What's behind a thread of Mercury: tapping an open question opens this. The
// question in full, its state, the salt it spiralled from (if any), and every
// past fire tended on it — each a door into that session's full transcript.
// Nothing about the thread is hidden behind a one-line row anymore.
struct ThreadDetailScreen: View {
    var model: AppModel
    let thread: Thread

    private var sessions: [SessionSummary] { model.engine.sessions(forThread: thread.id) }

    /// The salt this thread spiralled from (its lineage), when it was born of a
    /// realization — resolved from the grimoire, as the Mercury row does.
    private var spiraledFrom: String? {
        guard let pid = thread.parentRealizationId else { return nil }
        return model.engine.grimoire().first { $0.id == pid }?.text
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                question
                if let spiraledFrom { lineage(spiraledFrom) }
                pastFires
            }
            .padding(.horizontal, Ember.S.screenPad)
            .padding(.vertical, 20)
        }
        .background(Ember.C.ground.ignoresSafeArea())
        .navigationTitle("Mercury")
        .navigationBarTitleDisplayMode(.inline)
    }

    private var question: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                ThreadStateChip(thread: thread)
                Spacer()
                if !thread.domain.isEmpty {
                    Text(thread.domain)
                        .font(Ember.F.sans(10.5, weight: .bold))
                        .tracking(0.8)
                        .textCase(.uppercase)
                        .foregroundStyle(Ember.C.mutedDim)
                }
            }
            Text(thread.prompt)
                .font(Ember.F.serif(22))
                .foregroundStyle(Ember.C.ink)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.bottom, 4)
        .overlay(alignment: .bottom) { Ember.C.hairline.frame(height: 1) }
    }

    private func lineage(_ salt: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text(Ember.Glyph.grimoire).foregroundStyle(Ember.C.gold)
                Text("opened from")
                    .font(Ember.F.sans(10.5, weight: .bold))
                    .tracking(1.1)
                    .textCase(.uppercase)
                    .foregroundStyle(Ember.C.gold)
            }
            Text("\u{201C}\(salt)\u{201D}")
                .font(Ember.F.serif(15, italic: true))
                .foregroundStyle(Ember.C.muted)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder
    private var pastFires: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(sessions.isEmpty ? "no fires tended on this thread yet" : "fires tended on this thread")
                .font(Ember.F.sans(11, weight: .bold))
                .tracking(1.1)
                .textCase(.uppercase)
                .foregroundStyle(Ember.C.mutedDim)

            ForEach(sessions) { session in
                NavigationLink {
                    TranscriptView(model: model, sessionId: session.id)
                } label: {
                    SessionRow(session: session)
                }
                .buttonStyle(.plain)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.top, 4)
    }
}

/// One past fire in a history list — date, the mask it was worn in, and the
/// first line of what it left behind (its residue, or its trace). Tapping it
/// opens the full transcript. Shared by the thread detail and "past fires".
struct SessionRow: View {
    var session: SessionSummary

    private var dateLabel: String {
        let f = DateFormatter()
        f.dateFormat = "MMM d"
        return f.string(from: session.date).uppercased()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text(dateLabel)
                    .font(Ember.F.sans(11, weight: .bold))
                    .tracking(0.6)
                    .foregroundStyle(Ember.C.muted)
                Text(Ember.Glyph.fireMask)
                    .font(.system(size: 11))
                    .foregroundStyle(Ember.C.heat.opacity(0.8))
                Text(session.mask)
                    .font(Ember.F.sans(10.5, weight: .bold))
                    .tracking(0.8)
                    .textCase(.uppercase)
                    .foregroundStyle(Ember.C.mutedDim)
                Spacer()
                Text(Ember.Glyph.mercury)
                    .font(.system(size: 12))
                    .foregroundStyle(Ember.C.mutedDim)
            }
            Text(firstLine)
                .font(Ember.F.serif(15))
                .foregroundStyle(Ember.C.ink)
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Ember.C.raised, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).stroke(Ember.C.hairline, lineWidth: 1))
    }

    /// The first sentence-or-line of the residue, so the row reads at a glance.
    private var firstLine: String {
        let trimmed = session.excerpt.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return "a quiet fire — no residue set down" }
        return trimmed
    }
}

/// The thread's lifecycle state as a small chip — the same vocabulary the
/// Mercury row uses (ripe/volatile/condensing…), lifted here so the detail
/// header carries it without depending on MercuryScreen's private view.
struct ThreadStateChip: View {
    var thread: Thread

    private var label: String {
        if thread.isRipe { return "Ripe" }
        switch thread.state {
        case .volatile: return "Volatile"
        case .condensing: return "Condensing"
        case .fixed: return "Fixed"
        case .evaporated: return "Evaporated"
        }
    }

    var body: some View {
        Text(label)
            .font(Ember.F.sans(10, weight: .bold))
            .tracking(0.9)
            .textCase(.uppercase)
            .foregroundStyle(foreground)
            .padding(.horizontal, 10)
            .padding(.vertical, 3)
            .background(background, in: Capsule())
            .overlay(Capsule().stroke(border, lineWidth: border == .clear ? 0 : 1))
    }

    private var foreground: Color {
        if thread.isRipe { return Color(hex: 0x1c0f04) }
        return thread.state == .condensing ? Ember.C.heatHot : Ember.C.muted
    }
    private var background: Color {
        if thread.isRipe { return Ember.C.heat }
        return thread.state == .condensing ? Ember.C.heat.opacity(0.08) : Ember.C.raised
    }
    private var border: Color {
        if thread.isRipe { return .clear }
        return thread.state == .condensing ? Ember.C.heat.opacity(0.4) : Ember.C.hairline
    }
}
