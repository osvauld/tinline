import XCTest
@testable import Tinline

/// The profile rules, copy and ordered JSON (android has no JVM test for ProfileModel.kt; these pin the playground's messages).
final class ProfileModelTests: XCTestCase {
    private func obj(_ json: String) -> PeopleObject { PeopleJSON.parse(json)!.asObject! }

    func testNameChecks() {
        XCTAssertEqual(ProfileRules.checkName("  "), "The name can\u{2019}t be empty.")
        XCTAssertNotNil(ProfileRules.checkName("a\u{202E}b"))
        XCTAssertNotNil(ProfileRules.checkName(String(repeating: "a", count: 257)))
        XCTAssertNil(ProfileRules.checkName("Maya"))
    }

    func testTextAllowsNewlinesButNotControls() {
        XCTAssertNil(ProfileRules.checkText("a\nb", what: "about"))
        XCTAssertEqual(ProfileRules.checkText("a\u{07}b", what: "about"), "About can\u{2019}t contain control or text-direction characters.")
    }

    func testFieldChecksFollowProfileTsOrder() {
        let f = obj("{}")
        XCTAssertEqual(ProfileRules.checkField(key: "picture", value: "x", fields: f), "Use the picture controls for that.")
        XCTAssertEqual(ProfileRules.checkField(key: "availability", value: nil, fields: f), "Availability can be changed but not removed.")
        XCTAssertEqual(ProfileRules.checkField(key: " ", value: "x", fields: f), "Give the information a name.")
        XCTAssertNil(ProfileRules.checkField(key: "pronouns", value: "she/her", fields: f))
    }

    func testOrderedJsonKeepsInsertionOrderAndRoundTrips() {
        let o = obj("{\"zeta\":1,\"alpha\":\"x\",\"mid\":[true,null]}")
        XCTAssertEqual(o.keys, ["zeta", "alpha", "mid"])
        XCTAssertEqual(PeopleJSON.object(o).text, "{\"zeta\":1,\"alpha\":\"x\",\"mid\":[true,null]}")
        XCTAssertEqual(PeopleJSON.quote("a\"b\n"), "\"a\\\"b\\n\"")
        XCTAssertNil(PeopleJSON.parse("{oops"))
    }

    func testExtraKeysListKnownFirstThenAddedOrder() {
        let f = obj("{\"name\":\"M\",\"zeta\":1,\"website\":\"https://a.example/\",\"pronouns\":\"x\",\"alpha\":2}")
        XCTAssertEqual(PeopleProfileInfo.extraKeys(f), ["pronouns", "website", "zeta", "alpha"])
    }

    func testHumanizeAndLinks() {
        XCTAssertEqual(PeopleProfileInfo.humanize("work-title"), "Work title")
        XCTAssertEqual(PeopleProfileInfo.humanize("workTitle"), "Work Title")   // only the first letter is raised, as in info.ts
        XCTAssertEqual(PeopleProfileInfo.normaliseLink("arjun.example"), "https://arjun.example/")
        XCTAssertNil(PeopleProfileInfo.safeUrl("javascript:alert(1)"))
        XCTAssertNil(PeopleProfileInfo.safeUrl("https://user:pw@a.example/"))
        XCTAssertEqual(PeopleProfileInfo.shortUrl("https://arjun.example/"), "arjun.example")
    }

    func testAvailabilityPassedDeadlineReadsAvailable() {
        let f = obj("{\"availability\":{\"available\":false,\"until\":100}}")
        XCTAssertEqual(effectiveAvailability(f, nowMs: 50_000), Avail(available: false, untilMs: 100_000))
        XCTAssertEqual(effectiveAvailability(f, nowMs: 100_000), Avail(available: true))
        XCTAssertEqual(effectiveAvailability(nil), Avail(available: true))
    }

    func testBusyUntilSaysTodayTomorrowOrWeekday() {
        var cal = Calendar(identifier: .gregorian); cal.timeZone = TimeZone(identifier: "UTC")!
        let now: Int64 = 1_791_640_200_000   // 2026-10-10 13:50 UTC
        let en = Locale(identifier: "en_US")
        let clock = { (ms: Int64) -> String in
            let f = DateFormatter(); f.locale = en; f.calendar = cal; f.timeZone = cal.timeZone; f.dateStyle = .none; f.timeStyle = .short
            return f.string(from: Date(timeIntervalSince1970: Double(ms) / 1000))
        }
        XCTAssertEqual(PeopleProfileInfo.busyUntil(atMs: now + 3_600_000, nowMs: now, calendar: cal, locale: en), "Busy until " + clock(now + 3_600_000))
        XCTAssertEqual(PeopleProfileInfo.busyUntil(atMs: now + 86_400_000, nowMs: now, calendar: cal, locale: en), "Busy until tomorrow " + clock(now + 86_400_000))
        XCTAssertEqual(PeopleProfileInfo.busyUntil(atMs: now + 3 * 86_400_000, nowMs: now, calendar: cal, locale: en), "Busy until Tuesday " + clock(now + 3 * 86_400_000))
    }
}
