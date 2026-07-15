import SwiftUI

// Open threads (mirrors mockups-v2.html screen 6) — what will not be held.
// Ripe threads surface first; a swipe lets the learner drop a thread
// they've outgrown (evaporation is local/visual only here — the real
// evaporate write is the engine's, not this screen's, to make).
struct MercuryScreen: View {
    var model: AppModel

    @State private var evaporated: Set<String> = []

    private var threads: [Thread] {
        model.engine.mercury()
            .filter { !evaporated.contains($0.id) }
            .sorted { ($0.isRipe ? 0 : 1, $0.born) < ($1.isRipe ? 0 : 1, $1.born) }
    }

    /// Realization id → its salt text, for showing what a placeholder spiral
    /// child opened FROM instead of a wall of identical default questions.
    private var salts: [String: String] {
        Dictionary(
            model.engine.grimoire().map { ($0.id, $0.text) },
            uniquingKeysWith: { first, _ in first }
        )
    }

    /// The salt a thread spiralled from — only when its own prompt is still the
    /// bare placeholder (a real, authored question is shown as itself).
    private func spiraledFrom(_ thread: Thread) -> String? {
        guard thread.isPlaceholderSpiral, let pid = thread.parentRealizationId else { return nil }
        return salts[pid]
    }

    /// The "past fires" list — recent closed sessions regardless of thread, so
    /// threadless ones (initiation, bare tend-the-fire opens) are reachable too.
    private var pastFires: [SessionSummary] { model.engine.recentSessions(limit: 12) }

    var body: some View {
        // A NavigationStack so a thread row can push into what's behind it — the
        // thread's detail — and a past fire into its full transcript.
        NavigationStack {
            VStack(alignment: .leading, spacing: 0) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Mercury")
                        .font(Ember.F.serif(23, weight: .medium))
                        .foregroundStyle(Ember.C.ink)
                    Text("what will not be held · \(threads.count) volatile")
                        .font(Ember.F.sans(13))
                        .foregroundStyle(Ember.C.muted)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, Ember.S.screenPad)
                .padding(.top, 12)
                .padding(.bottom, 14)
                .overlay(alignment: .bottom) { Ember.C.hairline.frame(height: 1) }

                List {
                    ForEach(threads) { thread in
                        // Tapping a thread opens what's behind it — the question
                        // in full and every fire tended on it.
                        NavigationLink {
                            ThreadDetailScreen(model: model, thread: thread)
                        } label: {
                            ThreadRow(thread: thread, spiraledFrom: spiraledFrom(thread))
                        }
                        .listRowInsets(EdgeInsets(top: 0, leading: 0, bottom: 0, trailing: 0))
                        .listRowBackground(Ember.C.ground)
                        .listRowSeparatorTint(Ember.C.hairline)
                        .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                            Button(role: .destructive) {
                                withAnimation(Ember.Motion.none) { _ = evaporated.insert(thread.id) }
                            } label: {
                                Label("Evaporate", systemImage: "circle.dotted")
                            }
                            .tint(Ember.C.raised2)
                        }
                    }

                    if !pastFires.isEmpty {
                        Section {
                            ForEach(pastFires) { session in
                                NavigationLink {
                                    TranscriptView(model: model, sessionId: session.id)
                                } label: {
                                    SessionRow(session: session)
                                        .padding(.horizontal, Ember.S.screenPad)
                                        .padding(.vertical, 8)
                                }
                                .listRowInsets(EdgeInsets(top: 0, leading: 0, bottom: 0, trailing: 0))
                                .listRowBackground(Ember.C.ground)
                                .listRowSeparator(.hidden)
                            }
                        } header: {
                            Text("past fires · nothing lost")
                                .font(Ember.F.sans(11, weight: .bold))
                                .tracking(1.1)
                                .textCase(.uppercase)
                                .foregroundStyle(Ember.C.mutedDim)
                        }
                    }
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                .background(Ember.C.ground)
            }
            .background(Ember.C.ground)
            // Keep Mercury's own custom header; the empty root nav bar would
            // otherwise steal vertical space. Pushed screens set their own title.
            .toolbar(.hidden, for: .navigationBar)
        }
        .tint(Ember.C.heat)
    }
}

private struct ThreadRow: View {
    var thread: Thread
    /// Set only for a placeholder spiral child: the salt it opened from. When
    /// present, the row shows that lineage instead of the bare default question.
    var spiraledFrom: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            HStack(spacing: 10) {
                StateChip(thread: thread)
                Spacer()
                Text(thread.domain)
                    .font(Ember.F.sans(10.5, weight: .bold))
                    .tracking(0.8)
                    .textCase(.uppercase)
                    .foregroundStyle(Ember.C.mutedDim)
            }
            if let spiraledFrom {
                // A thread the Mystagogue hasn't renamed yet — show its lineage
                // (the salt it spiralled from) rather than a wall of identical
                // "what does this open?" placeholders.
                (Text("\(Ember.Glyph.grimoire)  opened from  ")
                    .foregroundStyle(Ember.C.mutedDim)
                    + Text("\u{201C}\(Self.excerpt(spiraledFrom))\u{201D}")
                    .foregroundStyle(Ember.C.muted))
                    .font(Ember.F.serif(15, italic: true))
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                Text(thread.prompt)
                    .font(Ember.F.serif(16.5))
                    .foregroundStyle(Ember.C.ink)
            }
            Text(ageLabel)
                .font(Ember.F.sans(12))
                .foregroundStyle(Ember.C.mutedDim)
                .monospacedDigit()
        }
        .padding(.horizontal, Ember.S.screenPad)
        .padding(.vertical, 18)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// A one-line excerpt of the parent salt: whitespace-collapsed, cut on a
    /// word boundary near ~72 chars with an ellipsis, so the lineage reads as a
    /// glance, not a paragraph.
    private static func excerpt(_ text: String, limit: Int = 72) -> String {
        let collapsed = text.split(whereSeparator: \.isWhitespace).joined(separator: " ")
        if collapsed.count <= limit { return collapsed }
        let clipped = collapsed.prefix(limit)
        let body = clipped.lastIndex(of: " ").map { String(clipped[..<$0]) } ?? String(clipped)
        return body.trimmingCharacters(in: .whitespaces) + "…"
    }

    private var ageLabel: String {
        let days = Calendar.current.dateComponents([.day], from: thread.born, to: Date()).day ?? 0
        let bornPart = days <= 0 ? "born today" : "born \(days)d ago"
        guard let lastWorked = thread.lastWorked else { return "\(bornPart) · not yet worked" }
        let workedDays = Calendar.current.dateComponents([.day], from: lastWorked, to: Date()).day ?? 0
        let workedPart = workedDays <= 0 ? "worked today" : "worked \(workedDays)d ago"
        return "\(bornPart) · \(workedPart)"
    }
}

private struct StateChip: View {
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
