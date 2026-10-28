import XCTest
@testable import Tinline

/** A call for another account of this device names it ("Asha → Work", docs/multi-online.md); the open account's does not. */
final class CallAccountTests: XCTestCase {
    private func info(_ name: String, account: AccountRef? = nil) -> CallInfo {
        CallInfo(callId: "c1", peerDid: "did:key:z6MkTestAsha", peerName: name, incoming: true, video: false, account: account)
    }

    func testOpenAccountShowsTheCaller() { XCTAssertEqual("Asha", info("Asha").ringName("Unknown")) }
    func testBlankCallerFallsBack() { XCTAssertEqual("Unknown", info("  ").ringName("Unknown")) }

    func testOtherAccountIsNamed() {
        let name = info("Asha", account: AccountRef(did: "did:key:z6MkTestWork", name: "Work")).ringName("Unknown")
        XCTAssertTrue(name.contains("Asha") && name.contains("Work"), name)
        XCTAssertNotEqual("call_for_account", name)
    }

    func testBlankCallerOnOtherAccountStillNamesIt() {
        let name = info("", account: AccountRef(did: "did:key:z6MkTestWork", name: "Work")).ringName("Unknown")
        XCTAssertTrue(name.contains("Unknown") && name.contains("Work"), name)
    }
}
