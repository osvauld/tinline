import XCTest
@testable import Tinline

final class CropTests: XCTestCase {
    func testCentredCoversTheViewportAndClampKeepsItCovered() {
        let c = centred(400, 200, view: 264)
        let s = coverScale(400, 200, view: 264, zoom: 1)
        XCTAssertEqual(s, 1.32, accuracy: 0.001)
        XCTAssertEqual(c.oy, 0, accuracy: 0.001)
        XCTAssertEqual(c.ox, (264 - 400 * s) / 2, accuracy: 0.001)
        let far = clampCrop(Crop(ox: 50, oy: -900, zoom: 1), 400, 200, view: 264)
        XCTAssertEqual(far.ox, 0)
        XCTAssertEqual(far.oy, 0, accuracy: 0.001)
    }

    func testZoomKeepsThePointUnderTheCentre() {
        let c = centred(400, 400, view: 264)
        let z = zoomAt(c, 2, 400, 400, view: 264)
        let s0 = coverScale(400, 400, view: 264, zoom: 1), s1 = coverScale(400, 400, view: 264, zoom: 2)
        XCTAssertEqual((132 - c.ox) / s0, (132 - z.ox) / s1, accuracy: 0.001)
    }

    func testSniffReadsTheFirstBytesNotTheName() {
        XCTAssertEqual(sniff(Data([0x89, 0x50, 0x4e, 0x47, 0, 0, 0, 0, 0, 0, 0, 0])), "png")
        XCTAssertEqual(sniff(Data([0xff, 0xd8, 0xff, 0xe0])), "jpeg")
        XCTAssertNil(sniff(Data("GIF89a".utf8)))
    }

    func testOversizedFileIsRefusedWithoutLoadingIt() {
        guard case .refused(let m) = inspectImage(Data(), size: 3 * 1024 * 1024) else { return XCTFail("expected refusal") }
        XCTAssertEqual(m, "That picture is 3 MB. Pictures can be up to 2 MB.")
    }
}
