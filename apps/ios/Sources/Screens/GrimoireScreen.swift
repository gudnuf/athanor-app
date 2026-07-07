import SwiftUI

// The salt shelf (mirrors mockups-v2.html screen 5) — chronological,
// immutable (no edit affordance anywhere in this file, ever), each grain
// carrying its domain(s) and the spiral link to the question it opened.
struct GrimoireScreen: View {
    var model: AppModel

    private var grains: [Realization] { model.engine.grimoire().sorted { $0.date > $1.date } }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 3) {
                Text("Grimoire")
                    .font(Ember.F.serif(23, weight: .medium))
                    .foregroundStyle(Ember.C.ink)
                Text("\(grains.count) grains · the salt shelf")
                    .font(Ember.F.sans(13))
                    .foregroundStyle(Ember.C.muted)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, Ember.S.screenPad)
            .padding(.top, 12)
            .padding(.bottom, 14)
            .overlay(alignment: .bottom) { Ember.C.hairline.frame(height: 1) }

            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(grains) { realization in
                        GrainRow(realization: realization, childPrompt: childPrompt(for: realization))
                    }
                }
                .padding(.horizontal, Ember.S.screenPad)
                .padding(.bottom, 100)
            }
        }
    }

    private func childPrompt(for realization: Realization) -> String? {
        guard let childId = realization.childThreadId else { return nil }
        return model.engine.mercury().first { $0.id == childId }?.prompt
    }
}

private struct GrainRow: View {
    var realization: Realization
    var childPrompt: String?

    private var dateLabel: String {
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM dd"
        return formatter.string(from: realization.date).uppercased()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 10) {
                Text(dateLabel)
                    .font(Ember.F.sans(11, weight: .bold))
                    .tracking(0.6)
                    .foregroundStyle(Ember.C.muted)
                Text(Ember.Glyph.grimoire)
                    .font(.system(size: 12))
                    .foregroundStyle(Ember.C.gold)
                Spacer()
                if let domain = realization.domains.first {
                    DomainChip(text: domain)
                }
            }
            Text(realization.text)
                .font(Ember.F.serif(16.5))
                .foregroundStyle(Ember.C.ink)
                .padding(.leading, 14)
                .overlay(alignment: .leading) {
                    Rectangle().fill(Ember.C.gold.opacity(0.4)).frame(width: 2)
                }
            if let childPrompt {
                Text("↳ opened: \(childPrompt)")
                    .font(Ember.F.serif(12.5, italic: true))
                    .foregroundStyle(Ember.C.mutedDim)
                    .padding(.leading, 14)
            }
        }
        .padding(.vertical, 20)
        .overlay(alignment: .bottom) { Ember.C.hairline.frame(height: 1) }
    }
}

struct DomainChip: View {
    var text: String

    var body: some View {
        Text(text)
            .font(Ember.F.sans(10.5, weight: .bold))
            .tracking(0.8)
            .textCase(.uppercase)
            .foregroundStyle(Ember.C.muted)
            .padding(.horizontal, 9)
            .padding(.vertical, 2)
            .overlay(Capsule().stroke(Ember.C.hairline, lineWidth: 1))
    }
}
