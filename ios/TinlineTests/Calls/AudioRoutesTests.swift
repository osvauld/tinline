import XCTest
@testable import Tinline

/** Port of android AudioRoutesTest.kt. */
final class AudioRoutesTests: XCTestCase {
    private let ear = AudioRoute(kind: .earpiece, name: "", key: "ear")
    private let spk = AudioRoute(kind: .speaker, name: "", key: "spk")
    private let bt = AudioRoute(kind: .bluetooth, name: "Buds", key: "bt")
    private let wired = AudioRoute(kind: .wired, name: "", key: "wired")

    func testOneToOneStartsOnEarpieceWithoutHeadset() {
        XCTAssertEqual(ear, CallRoutes.pickDefault([spk, ear], preferSpeaker: false))
    }

    func testGroupAndVideoStartOnSpeakerWithoutHeadset() {
        XCTAssertEqual(spk, CallRoutes.pickDefault([ear, spk], preferSpeaker: true))
    }

    func testHeadsetWinsOverSpeakerPreference() {
        XCTAssertEqual(bt, CallRoutes.pickDefault([ear, spk, wired, bt], preferSpeaker: true))
        XCTAssertEqual(wired, CallRoutes.pickDefault([ear, spk, wired], preferSpeaker: true))
    }

    func testNoRoutesPicksNothing() {
        XCTAssertNil(CallRoutes.pickDefault([], preferSpeaker: false))
    }

    func testToggleGoesToSpeakerAndBackToHeadset() {
        XCTAssertEqual(spk, CallRoutes.toggleTarget(RouteState(current: bt, available: [ear, spk, bt])))
        XCTAssertEqual(bt, CallRoutes.toggleTarget(RouteState(current: spk, available: [ear, spk, bt])))
        XCTAssertEqual(ear, CallRoutes.toggleTarget(RouteState(current: spk, available: [ear, spk])))
    }

    func testStateFlags() {
        XCTAssertTrue(RouteState(current: ear, available: [ear, spk]).earpiece)
        XCTAssertFalse(RouteState(current: ear, available: [ear, spk]).hasHeadset)
        XCTAssertTrue(RouteState(current: spk, available: [ear, spk, bt]).hasHeadset)
        XCTAssertTrue(RouteState(current: spk, available: [ear, spk]).speaker)
        XCTAssertFalse(RouteState().earpiece)
    }

    func testGenericBluetoothNameCountsAsUnnamed() {
        var generic = bt; generic.name = CallRoutes.genericBluetooth
        var blank = bt; blank.name = ""
        XCTAssertTrue(CallRoutes.unnamedBluetooth(RouteState(current: generic, available: [ear, spk, generic])))
        XCTAssertTrue(CallRoutes.unnamedBluetooth(RouteState(current: ear, available: [ear, blank])))
        XCTAssertFalse(CallRoutes.unnamedBluetooth(RouteState(current: bt, available: [ear, spk, bt])))
        XCTAssertFalse(CallRoutes.unnamedBluetooth(RouteState(current: ear, available: [ear, spk])))
        XCTAssertEqual("Bluetooth", CallRoutes.label(generic))
    }

    func testVideoCallIsPlacedAsVideoThenAnyCameraOn() {
        // Ringing/dialling: how it was placed.
        XCTAssertTrue(CallRoutes.isVideo(placedAsVideo: true, decided: false, negotiated: false, localCamera: false, remoteCamera: false))
        XCTAssertFalse(CallRoutes.isVideo(placedAsVideo: false, decided: false, negotiated: false, localCamera: false, remoteCamera: false))
        // My camera on while it rings ("Video") decides nothing yet.
        XCTAssertFalse(CallRoutes.isVideo(placedAsVideo: false, decided: false, negotiated: false, localCamera: true, remoteCamera: false))
        // Active: video runs and a camera is on.
        XCTAssertTrue(CallRoutes.isVideo(placedAsVideo: true, decided: true, negotiated: true, localCamera: true, remoteCamera: false))
        XCTAssertFalse(CallRoutes.isVideo(placedAsVideo: false, decided: true, negotiated: true, localCamera: false, remoteCamera: false))
        XCTAssertTrue(CallRoutes.isVideo(placedAsVideo: false, decided: true, negotiated: true, localCamera: true, remoteCamera: false))
        XCTAssertTrue(CallRoutes.isVideo(placedAsVideo: false, decided: true, negotiated: true, localCamera: false, remoteCamera: true))
        // Placed as video to a device without video: a voice call once that is known.
        XCTAssertFalse(CallRoutes.isVideo(placedAsVideo: true, decided: true, negotiated: false, localCamera: true, remoteCamera: false))
    }

    func testBecomingVideoIsOncePerChange() {
        XCTAssertTrue(CallRoutes.becameVideo(wasVideo: false, isVideo: true))
        // A call that already is video is left where the person put it.
        XCTAssertFalse(CallRoutes.becameVideo(wasVideo: true, isVideo: true))
        XCTAssertFalse(CallRoutes.becameVideo(wasVideo: false, isVideo: false))
        XCTAssertFalse(CallRoutes.becameVideo(wasVideo: true, isVideo: false))
    }

    func testProximityOnlyForVoiceOnEarpiece() {
        let onEar = RouteState(current: ear, available: [ear, spk])
        XCTAssertTrue(CallRoutes.proximityWanted(onEar, video: false))
        XCTAssertFalse(CallRoutes.proximityWanted(onEar, video: true))
        XCTAssertFalse(CallRoutes.proximityWanted(RouteState(current: spk, available: [ear, spk]), video: false))
        XCTAssertFalse(CallRoutes.proximityWanted(RouteState(), video: false))
    }

    func testLabels() {
        var blank = bt; blank.name = ""
        XCTAssertEqual("Phone", CallRoutes.label(ear))
        XCTAssertEqual("Buds", CallRoutes.label(bt))
        XCTAssertEqual("Bluetooth", CallRoutes.label(blank))
        XCTAssertEqual("Headset", CallRoutes.label(wired))
    }

    func testRouteSlotShowsTheHeadsetInUse() {
        XCTAssertEqual("Speaker", CallRoutes.slot(RouteState(current: ear, available: [ear, spk])).caption)
        XCTAssertEqual("Buds", CallRoutes.slot(RouteState(current: bt, available: [ear, spk, bt])).caption)
        XCTAssertEqual("Speaker", CallRoutes.slot(RouteState(current: spk, available: [ear, spk, bt])).caption)
        XCTAssertEqual("Speaker", CallRoutes.slot(nil).caption)
    }
}
