import SwiftUI
import UIKit

struct ShareSheet: UIViewControllerRepresentable {
    let items: [URL]
    /// Called when the activity completes or is cancelled; the host dismisses the sheet,
    /// and its `onDismiss` does the (single) temp cleanup.
    var onFinish: () -> Void = {}
    func makeUIViewController(context: Context) -> UIActivityViewController {
        let vc = UIActivityViewController(activityItems: items, applicationActivities: nil)
        vc.completionWithItemsHandler = { _, _, _, _ in onFinish() }
        return vc
    }
    func updateUIViewController(_ vc: UIActivityViewController, context: Context) {}
}

struct VaultShare: Identifiable { let id = UUID(); let urls: [URL] }
