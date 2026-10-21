import Foundation

/*
 * Profile rules and copy, ported from playground src/lib/model/actions/profile.ts, screens/profile/info.ts and copy.ts
 * (android ProfileModel.kt). Messages are the playground's, verbatim. Fields are data: nothing here fetches, embeds or runs
 * anything a field contains.
 */

/// A check's outcome: nil is fine, otherwise the message to show.
enum ProfileRules {
    static let nameLimit = 256
    static let textLimit = 4096
    static let fields = 64
    static let valueLimit = 16 * 1024
    static let allLimit = 64 * 1024
    static let keyLimit = 256
    static let reserved: Set<String> = ["name", "status", "about", "availability", "picture"]

    static func utf8(_ s: String) -> Int { s.utf8.count }

    private static func hasControl(_ s: String) -> Bool {
        s.unicodeScalars.contains { (0x00...0x1f).contains($0.value) || (0x7f...0x9f).contains($0.value) }
    }
    private static func hasBidi(_ s: String) -> Bool {
        s.unicodeScalars.contains { (0x202a...0x202e).contains($0.value) || (0x2066...0x2069).contains($0.value) }
    }
    /// Text may span lines: tab, LF and CR are allowed.
    private static func hasTextControl(_ s: String) -> Bool {
        s.unicodeScalars.contains { let v = $0.value; return (0x00...0x08).contains(v) || v == 0x0b || v == 0x0c || (0x0e...0x1f).contains(v) || v == 0x7f }
    }

    static func checkName(_ raw: String, what: String = "name") -> String? {
        let s = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if s.isEmpty { return "The \(what) can\u{2019}t be empty." }
        if hasControl(s) || hasBidi(s) { return "The \(what) can\u{2019}t contain control or text-direction characters." }
        if utf8(s) > nameLimit { return "That \(what) is too long. Up to \(nameLimit) bytes; some characters take more than one." }
        return nil
    }

    static func checkText(_ s: String, what: String) -> String? {
        if utf8(s) > textLimit { return "That is too long for \(what): up to \(textLimit) bytes, and some characters take more than one." }
        if hasTextControl(s) || hasBidi(s) { return what.prefix(1).uppercased() + what.dropFirst() + " can\u{2019}t contain control or text-direction characters." }
        return nil
    }

    static func checkKey(_ key: String) -> String? {
        if key.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return "Give the information a name." }
        if hasControl(key) || utf8(key) > keyLimit { return "That name can\u{2019}t be used. Keep it short, without control characters." }
        return nil
    }

    /// profile.ts checkValue: per value 16 KiB, 64 fields, 64 KiB in all. `json` is the value's JSON text.
    static func checkValue(key: String, json: String, fields obj: PeopleObject) -> String? {
        let size = utf8(json)
        if size > valueLimit { return "That is too long. Each piece of information can be up to 16 KiB." }
        let others = obj.keys.filter { $0 != key }
        if others.count >= fields && !obj.has(key) { return "You\u{2019}ve reached 64 pieces of information. Remove one to add another." }
        let total = others.reduce(0) { $0 + utf8($1) + utf8((obj[$1] ?? .null).text) } + utf8(key) + size
        if total > allLimit { return "Your profile is full. Remove some information to add more." }
        return nil
    }

    /// The full pre-check for `setField(key, value)`, in profile.ts's order. A nil value removes.
    static func checkField(key: String, value: String?, fields obj: PeopleObject) -> String? {
        if let m = checkKey(key) { return m }
        if key == "name" { return value.flatMap { checkName($0) } }
        if key == "picture" { return "Use the picture controls for that." }
        guard let value else { return key == "availability" ? "Availability can be changed but not removed." : nil }
        if key == "status" || key == "about" {
            if let m = checkText(value, what: key == "status" ? "a status" : "about") { return m }
        }
        return checkValue(key: key, json: PeopleJSON.quote(value), fields: obj)
    }
}

/// info.ts KNOWN: keys with their own control.
struct KnownField {
    let label: String
    let link: Bool
    let hint: String
}

enum ProfileInfo {
    /// In display order.
    static let knownKeys = ["pronouns", "location", "website"]
    static let known: [String: KnownField] = [
        "pronouns": KnownField(label: "Pronouns", link: false, hint: "For example, she/her"),
        "location": KnownField(label: "Location", link: false, hint: "City or region"),
        "website": KnownField(label: "Website", link: true, hint: "https://"),
    ]

    /// "work-title" / "work_title" -> "Work title". The stored key is never changed.
    static func humanize(_ key: String) -> String {
        var s = key.replacingOccurrences(of: "[-_.]+", with: " ", options: .regularExpression)
        s = s.replacingOccurrences(of: "([a-z])([A-Z])", with: "$1 $2", options: .regularExpression)
        s = s.trimmingCharacters(in: .whitespacesAndNewlines)
        return s.isEmpty ? key : s.prefix(1).uppercased() + s.dropFirst()
    }

    static func labelOf(_ key: String) -> String { known[key]?.label ?? humanize(key) }

    /// Additional-information keys in display order: known ones first, then the rest in the order they were added.
    static func extraKeys(_ fields: PeopleObject?) -> [String] {
        guard let fields else { return [] }
        let keys = fields.keys.filter { !ProfileRules.reserved.contains($0) }
        return knownKeys.filter { keys.contains($0) } + keys.filter { known[$0] == nil }
    }

    /// http(s) only, no embedded credentials. Anything else is shown as plain text and never offered as a link.
    static func safeUrl(_ raw: String?) -> String? {
        guard let raw, let u = URLComponents(string: raw.trimmingCharacters(in: .whitespacesAndNewlines)) else { return nil }
        let scheme = u.scheme?.lowercased()
        guard scheme == "https" || scheme == "http", u.user == nil, u.password == nil, let host = u.host, !host.isEmpty else { return nil }
        // As URL.href: lower-case scheme and host, an empty path becomes "/".
        let port = u.port.map { ":\($0)" } ?? ""
        let path = u.percentEncodedPath.isEmpty ? "/" : u.percentEncodedPath
        return "\(scheme!)://\(host.lowercased())\(port)\(path)"
            + (u.percentEncodedQuery.map { "?\($0)" } ?? "") + (u.percentEncodedFragment.map { "#\($0)" } ?? "")
    }

    /// What the user typed -> a stored address. A bare "arjun.example" becomes https://arjun.example/.
    static func normaliseLink(_ raw: String) -> String? {
        let t = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if t.isEmpty { return nil }
        let hasScheme = t.range(of: "^[a-z][a-z0-9+.-]*:", options: [.regularExpression, .caseInsensitive]) != nil
        return safeUrl(hasScheme ? t : "https://\(t)")
    }

    static func shortUrl(_ u: String) -> String {
        var s = u.replacingOccurrences(of: "^https?://", with: "", options: .regularExpression)
        if s.hasSuffix("/") { s.removeLast() }
        return s
    }

    /// "18:00", "tomorrow 08:00", "Friday 08:00": a deadline as people say it. `atMs`, `nowMs` in milliseconds.
    static func untilLabel(atMs: Int64, nowMs: Int64 = nowMillis(), calendar: Calendar = .current, locale: Locale = .current) -> String {
        let a = Date(timeIntervalSince1970: Double(atMs) / 1000)
        let now = Date(timeIntervalSince1970: Double(nowMs) / 1000)
        let c = calendar.dateComponents([.hour, .minute], from: a)
        let hm = String(format: "%02d:%02d", c.hour ?? 0, c.minute ?? 0)
        if calendar.isDate(a, inSameDayAs: now) { return hm }
        if let tomorrow = calendar.date(byAdding: .day, value: 1, to: now), calendar.isDate(a, inSameDayAs: tomorrow) { return "tomorrow \(hm)" }
        let f = DateFormatter()
        f.locale = locale; f.calendar = calendar; f.timeZone = calendar.timeZone; f.dateFormat = "EEEE"
        return "\(f.string(from: a)) \(hm)"
    }

    static func nowMillis() -> Int64 { Int64(Date().timeIntervalSince1970 * 1000) }
}

/// profile.ts BUSY_CHOICES; `until` in milliseconds (nil = no end time).
struct BusyChoice {
    let id: String
    let label: String
    let until: (Int64) -> Int64?
}

let busyChoices: [BusyChoice] = [
    BusyChoice(id: "30m", label: "30 minutes") { $0 + 30 * 60_000 },
    BusyChoice(id: "1h", label: "1 hour") { $0 + 3_600_000 },
    BusyChoice(id: "4h", label: "4 hours") { $0 + 4 * 3_600_000 },
    BusyChoice(id: "tom", label: "Tomorrow morning") { now in
        let cal = Calendar.current
        let d = Date(timeIntervalSince1970: Double(now) / 1000)
        guard let next = cal.date(byAdding: .day, value: 1, to: cal.startOfDay(for: d)),
              let eight = cal.date(bySettingHour: 8, minute: 0, second: 0, of: next) else { return now + 12 * 3_600_000 }
        return Int64(eight.timeIntervalSince1970 * 1000)
    },
    BusyChoice(id: "open", label: "No end time") { _ in nil },
]

/// The availability field ({available, until?} with until in unix seconds) at this moment: a passed deadline is Available again.
struct Avail: Equatable {
    var available: Bool
    var untilMs: Int64? = nil
}

func effectiveAvailability(_ fields: PeopleObject?, nowMs: Int64 = ProfileInfo.nowMillis()) -> Avail {
    guard let a = fields?["availability"]?.asObject else { return Avail(available: true) }
    if case .bool(let b)? = a["available"], !b {} else { return Avail(available: true) }
    var until: Int64?
    if case .number(let n)? = a["until"], let d = Double(n) { until = Int64(d) * 1000 }
    if let until, until <= nowMs { return Avail(available: true) }
    return Avail(available: false, untilMs: until)
}

/// copy.ts, verbatim.
let pictureHelp = "Replacing or removing your picture requests deletion of the previous cached copy on your linked devices and contacts when they sync. Screenshots, backups, and copies retained by modified clients cannot be erased by the app."
let fieldHelp = "Your contacts can see your profile. Removing information hides it from your profile; earlier versions can remain in synced history."

/// ProfilePolicy.kt: Busy is not honoured by the core's call path yet, so the UI refuses it instead of pretending.
enum ProfilePolicy {
    static let busyUnsupported = "Busy is not available yet. Your call settings are unchanged."
    static func availabilityProblem(available: Bool) -> String? { available ? nil : busyUnsupported }
}
