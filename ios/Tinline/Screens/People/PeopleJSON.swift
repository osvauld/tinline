import Foundation

/*
 * An insertion-ordered JSON value. The profile's `fieldsJson` is an extensible object and the screens show "additional
 * information" in the order it was added (Android's org.json keeps insertion order). JSONSerialization does not, so this is a
 * small parser of our own. Fields are data: nothing here fetches or runs anything a field contains.
 */

struct PeopleObject: Equatable {
    private(set) var keys: [String] = []
    private var map: [String: PeopleJSON] = [:]

    init() {}
    func has(_ key: String) -> Bool { map[key] != nil }
    subscript(key: String) -> PeopleJSON? {
        get { map[key] }
        set {
            if let v = newValue { if map[key] == nil { keys.append(key) }; map[key] = v }
            else { map[key] = nil; keys.removeAll { $0 == key } }
        }
    }
}

indirect enum PeopleJSON: Equatable {
    case null
    case bool(Bool)
    /// The number as written, so it shows exactly as stored (Kotlin `Number.toString()`).
    case number(String)
    case string(String)
    case array([PeopleJSON])
    case object(PeopleObject)

    var asString: String? { if case .string(let s) = self { return s }; return nil }
    var asObject: PeopleObject? { if case .object(let o) = self { return o }; return nil }
    var isNull: Bool { if case .null = self { return true }; return false }
    /// info.ts isScalar: text, numbers and booleans.
    var isScalar: Bool {
        switch self { case .string, .number, .bool: return true; default: return false }
    }

    /// Parses any JSON text; nil when it is not valid JSON.
    static func parse(_ text: String) -> PeopleJSON? {
        var p = PeopleJSONParser(Array(text.utf16))
        guard let v = p.value() else { return nil }
        p.skipSpace()
        return p.atEnd ? v : nil
    }

    /// Compact JSON text (used for sizes and for `set_profile_field`).
    var text: String {
        switch self {
        case .null: return "null"
        case .bool(let b): return b ? "true" : "false"
        case .number(let n): return n
        case .string(let s): return PeopleJSON.quote(s)
        case .array(let a): return "[" + a.map { $0.text }.joined(separator: ",") + "]"
        case .object(let o): return "{" + o.keys.map { PeopleJSON.quote($0) + ":" + (o[$0] ?? .null).text }.joined(separator: ",") + "}"
        }
    }

    /// `JSONObject.quote`: a JSON string literal.
    static func quote(_ s: String) -> String {
        var out = "\""
        for u in s.unicodeScalars {
            switch u {
            case "\"": out += "\\\""
            case "\\": out += "\\\\"
            case "\n": out += "\\n"
            case "\r": out += "\\r"
            case "\t": out += "\\t"
            case "\u{08}": out += "\\b"
            case "\u{0C}": out += "\\f"
            default:
                if u.value < 0x20 { out += String(format: "\\u%04x", u.value) } else { out.unicodeScalars.append(u) }
            }
        }
        return out + "\""
    }
}

private struct PeopleJSONParser {
    let s: [UInt16]
    var i = 0
    init(_ s: [UInt16]) { self.s = s }
    var atEnd: Bool { i >= s.count }

    mutating func skipSpace() { while i < s.count, [0x20, 0x09, 0x0A, 0x0D].contains(s[i]) { i += 1 } }
    private func ch(_ c: Character) -> UInt16 { c.utf16.first! }

    mutating func value() -> PeopleJSON? {
        skipSpace()
        guard i < s.count else { return nil }
        switch s[i] {
        case ch("{"): return object()
        case ch("["): return array()
        case ch("\""): return string().map { .string($0) }
        case ch("t"): return literal("true", .bool(true))
        case ch("f"): return literal("false", .bool(false))
        case ch("n"): return literal("null", .null)
        default: return number()
        }
    }

    private mutating func literal(_ word: String, _ v: PeopleJSON) -> PeopleJSON? {
        let w = Array(word.utf16)
        guard i + w.count <= s.count, Array(s[i..<i + w.count]) == w else { return nil }
        i += w.count
        return v
    }

    private mutating func number() -> PeopleJSON? {
        let start = i
        while i < s.count, "+-0123456789.eE".utf16.contains(s[i]) { i += 1 }
        guard i > start else { return nil }
        let t = String(decoding: s[start..<i], as: UTF16.self)
        guard Double(t) != nil else { return nil }
        return .number(t)
    }

    private mutating func string() -> String? {
        guard i < s.count, s[i] == ch("\"") else { return nil }
        i += 1
        var units: [UInt16] = []
        while i < s.count {
            let c = s[i]; i += 1
            if c == ch("\"") { return String(decoding: units, as: UTF16.self) }
            if c != ch("\\") { units.append(c); continue }
            guard i < s.count else { return nil }
            let e = s[i]; i += 1
            switch e {
            case ch("n"): units.append(0x0A)
            case ch("r"): units.append(0x0D)
            case ch("t"): units.append(0x09)
            case ch("b"): units.append(0x08)
            case ch("f"): units.append(0x0C)
            case ch("u"):
                guard i + 4 <= s.count, let n = UInt16(String(decoding: s[i..<i + 4], as: UTF16.self), radix: 16) else { return nil }
                units.append(n); i += 4
            default: units.append(e)   // \" \\ \/
            }
        }
        return nil
    }

    private mutating func array() -> PeopleJSON? {
        i += 1
        var items: [PeopleJSON] = []
        skipSpace()
        if i < s.count, s[i] == ch("]") { i += 1; return .array(items) }
        while true {
            guard let v = value() else { return nil }
            items.append(v)
            skipSpace()
            guard i < s.count else { return nil }
            if s[i] == ch(",") { i += 1; continue }
            if s[i] == ch("]") { i += 1; return .array(items) }
            return nil
        }
    }

    private mutating func object() -> PeopleJSON? {
        i += 1
        var o = PeopleObject()
        skipSpace()
        if i < s.count, s[i] == ch("}") { i += 1; return .object(o) }
        while true {
            skipSpace()
            guard let k = string() else { return nil }
            skipSpace()
            guard i < s.count, s[i] == ch(":") else { return nil }
            i += 1
            guard let v = value() else { return nil }
            o[k] = v
            skipSpace()
            guard i < s.count else { return nil }
            if s[i] == ch(",") { i += 1; continue }
            if s[i] == ch("}") { i += 1; return .object(o) }
            return nil
        }
    }
}
