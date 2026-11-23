import Foundation

/// Hidden-door vault export (docs/gestures.md): writes core's markdown and a
/// core-made db copy into a fresh temp dir and returns the files to share.
/// No logic — core renders everything; this only names files and writes bytes.
enum VaultExport {
    static var directory: URL {
        FileManager.default.temporaryDirectory.appendingPathComponent("vault-export", isDirectory: true)
    }

    static func prepare() -> [URL] {
        let fm = FileManager.default
        let dir = directory
        try? fm.removeItem(at: dir)
        guard (try? fm.createDirectory(at: dir, withIntermediateDirectories: true)) != nil else { return [] }
        let f = DateFormatter(); f.dateFormat = "yyyy-MM-dd"; f.locale = Locale(identifier: "en_US_POSIX")
        let stamp = f.string(from: Date())
        var urls: [URL] = []
        if let md = RealEngineLoader.exportVault(), let data = md.data(using: .utf8) {
            let u = dir.appendingPathComponent("athanor-vault-\(stamp).md")
            if (try? data.write(to: u)) != nil { urls.append(u) }
        }
        let db = dir.appendingPathComponent("athanor-\(stamp).sqlite")
        if RealEngineLoader.exportDatabaseCopy(to: db.path) { urls.append(db) }
        // Sizes only — never content (private learner data).
        let sizes = urls.map { (try? fm.attributesOfItem(atPath: $0.path)[.size] as? Int) ?? 0 }
        NSLog("[Athanor] vault export prepared: \(urls.count) file(s), bytes=\(sizes)")
        return urls
    }

    /// Private data must not linger in tmp after the sheet closes (review N9).
    static func cleanUp() { try? FileManager.default.removeItem(at: directory) }
}
