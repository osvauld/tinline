import Combine
import Foundation

/**
 * The calls area's object graph, made once by the Shell next to [AppEnv] (P2pApp.kt builds the same pieces):
 * the 1:1 [controller], its [video], the group call's [groupAudio] and [groupUi], and the [hostConditions] pushed to the
 * core. [attach] registers the core callbacks this area implements (`VideoEvents`, `GroupCallEvents`) on a new Node;
 * the Shell's `NodeEvents` forwards `onIncomingCall` / `onCallState` to [onIncomingCall] / [onCallState].
 */
@MainActor
final class CallsEnv: ObservableObject {
    let controller: CallController
    let video: CallVideo
    let groupAudio: GroupCallAudio
    let groupUi: GroupCallUi
    let hostConditions: HostConditions
    let platform: CallPlatform
    let directory: CallDirectory

    /** The node as the core callbacks and blocking calls see it; replaced on [attach]. */
    private let box: CoreBox
    private var bag: Set<AnyCancellable> = []

    /// Defaults are built in the body, not as default arguments: those run nonisolated and the stubs are
    /// main-actor types. `provider` nil means no CallKit provider (the Shell passes the stub; tests pass none).
    init(platform: CallPlatform? = nil, alerts: CallAlerts? = nil, directory: CallDirectory? = nil,
         provider: CallProvider? = nil, camera: CallCamera? = nil, readings: HostReadingSource? = nil) {
        let platform = platform ?? CallPlatformIOS()
        let alerts = alerts ?? CallAlertsStub()
        let directory = directory ?? CallDirectoryStub()
        let camera = camera ?? AVCallCamera()
        let readings = readings ?? DeviceHostReadings()
        let box = CoreBox()
        let core: @Sendable () -> CallCore? = { box.get() }
        self.box = box
        self.platform = platform
        self.directory = directory
        controller = CallController(core: core, platform: platform, alerts: alerts, directory: directory, provider: provider)
        video = CallVideo(core: core, platform: platform, directory: directory, camera: camera)
        groupAudio = GroupCallAudio(core: core, platform: platform, provider: provider)
        groupUi = GroupCallUi(core: core, audio: groupAudio, platform: platform, alerts: alerts, directory: directory)
        hostConditions = HostConditions(core: core, source: readings)
        controller.video = video
        video.controller = controller
        video.metered = { [weak self] in self?.hostConditions.metered ?? false }
        groupUi.oneToOneBusy = { [weak self] in self?.controller.ui != nil }
        // Redraw whoever observes the env when any part changes (the strip reads several).
        for p in [controller.objectWillChange, video.objectWillChange, groupUi.objectWillChange, groupAudio.objectWillChange] {
            p.sink { [weak self] _ in self?.objectWillChange.send() }.store(in: &bag)
        }
    }

    /** A new Node (unlock, account switch): register the call callbacks on it and start the host conditions. */
    func attach(_ node: Node) {
        useCore(node)
        node.setVideoEvents(events: video)
        node.setGroupCallEvents(events: groupAudio)
        hostConditions.start()
    }

    /** The core every part reads; [attach] sets it for a Node, tests set a fake. */
    func useCore(_ core: CallCore?) { box.set(core) }

    /** The node is closing (lock, account switch). */
    func detach() {
        hostConditions.stop()
        useCore(nil)
    }

    func onIncomingCall(_ call: CallInfo) { controller.onIncoming(call) }
    func onCallState(_ callId: String, _ state: CallState) { controller.onState(callId, state) }

    /** The app left the front: a ringing call (1:1 or group) gets its alert now. */
    func appLeftFront() { controller.appLeftFront(); groupUi.appLeftFront() }

    /** In-call title for this device, as the Android ongoing notification derives it. */
    func inCallState() -> CallNotif.Derived {
        let u = controller.ui
        let here = groupAudio.here
        return CallNotif.derive(call: CallNotif.of(u), callName: u.map { callOr($0.info.peerName, tr("call_notification_name")) } ?? tr("call_notification_name"),
                                groupInCall: here?.inCall == true, groupHosting: here?.hosting == true,
                                groupName: groupAudio.callName(), preparedAtMs: groupAudio.preparedAtMs, nowMs: callNowMs())
    }
}

/** The current node behind a lock, readable from any thread (the audio loops, core callbacks). */
private final class CoreBox: @unchecked Sendable {
    private let lock = NSLock()
    private var node: CallCore?
    func get() -> CallCore? { lock.lock(); defer { lock.unlock() }; return node }
    func set(_ n: CallCore?) { lock.lock(); node = n; lock.unlock() }
}
