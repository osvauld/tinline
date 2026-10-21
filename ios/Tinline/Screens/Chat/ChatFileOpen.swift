import Foundation

/// How a received file may be opened. Decided from BOTH the declared MIME and the extension, because both come from the
/// other person. (android FileOpen.kt)
enum ChatOpenKind { case pdf, text, external, unsupported }

/// `mime` is what we pass to other apps: derived from the extension, never the sender's claim.
struct ChatOpenPlan: Equatable { let kind: ChatOpenKind; let mime: String }

enum ChatFileOpen {
    static let textLimit: Int64 = 1_000_000

    private static let textExt: Set<String> = ["txt", "md", "csv", "log"]
    private static let externalMime: [String: String] = [
        "doc": "application/msword", "docx": "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls": "application/vnd.ms-excel", "xlsx": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt": "application/vnd.ms-powerpoint", "pptx": "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt": "application/vnd.oasis.opendocument.text", "ods": "application/vnd.oasis.opendocument.spreadsheet",
        "odp": "application/vnd.oasis.opendocument.presentation", "rtf": "application/rtf", "epub": "application/epub+zip",
        "mp3": "audio/mpeg", "m4a": "audio/mp4", "aac": "audio/aac", "ogg": "audio/ogg", "opus": "audio/ogg", "wav": "audio/x-wav", "flac": "audio/flac",
        "mp4": "video/mp4", "m4v": "video/mp4", "mkv": "video/x-matroska", "webm": "video/webm", "mov": "video/quicktime", "3gp": "video/3gpp",
    ]
    private static let generic: Set<String> = ["", "application/octet-stream", "binary/octet-stream", "application/x-binary", "application/unknown"]
    private static let blockedMime: Set<String> = ["application/vnd.android.package-archive", "application/x-apk", "application/xapk-package-archive"]
    private static let textMime: Set<String> = ["text/plain", "text/markdown", "text/x-markdown", "text/csv", "text/comma-separated-values", "text/x-log"]
    private static let officeMime: Set<String> = ["application/msword", "application/vnd.ms-excel", "application/vnd.ms-powerpoint", "application/rtf", "text/rtf", "application/epub+zip"]

    static func extOf(_ name: String) -> String {
        guard let dot = name.lastIndex(of: ".") else { return "" }
        return String(name[name.index(after: dot)...]).lowercased()
    }

    private static func extKind(_ ext: String) -> ChatOpenKind {
        if ext == "pdf" { return .pdf }
        if textExt.contains(ext) { return .text }
        if externalMime[ext] != nil { return .external }
        return .unsupported
    }

    /// nil = the MIME says nothing (generic); `.unsupported` = it names something that is not on the list.
    private static func mimeKind(_ mime: String) -> ChatOpenKind? {
        let m = (mime.split(separator: ";", maxSplits: 1, omittingEmptySubsequences: false).first.map(String.init) ?? "")
            .trimmingCharacters(in: .whitespaces).lowercased()
        if generic.contains(m) { return nil }
        if blockedMime.contains(m) { return ChatOpenKind.unsupported }
        if m == "application/pdf" { return .pdf }
        if textMime.contains(m) { return .text }
        if m.hasPrefix("audio/") || m.hasPrefix("video/") { return .external }
        if m.hasPrefix("application/vnd.openxmlformats-officedocument.") || m.hasPrefix("application/vnd.oasis.opendocument.") || officeMime.contains(m) { return .external }
        return ChatOpenKind.unsupported
    }

    static func plan(name: String, mime: String) -> ChatOpenPlan {
        let none = ChatOpenPlan(kind: .unsupported, mime: "application/octet-stream")
        let ext = extOf(name)
        if ["apk", "apks", "xapk", "apkm"].contains(ext) { return none }
        let e = extKind(ext)
        if e == .unsupported { return none }
        if let m = mimeKind(mime), m != e { return none }
        let out: String
        switch e {
        case .pdf: out = "application/pdf"
        case .text: out = "text/plain"
        default: out = externalMime[ext] ?? "application/octet-stream"
        }
        return ChatOpenPlan(kind: e, mime: out)
    }
}
