import AVFoundation
import CoreImage
import CoreImage.CIFilterBuiltins
import LocalAuthentication
import Network
import SwiftUI
import UIKit
import UserNotifications

/*
 * The platform pieces the Shell needs, each behind a small protocol so previews and tests can swap them, and so the parts
 * that cannot be checked without a device are in one place (TODO(ios-device) marks them).
 */

// ---------------------------------------------------------------- remembered key (android UnlockStore.kt)

/// What a remembered-key read found.
enum ShellKeyLoad {
    case key(Data)
    /// Definitively unusable (the item is gone or cannot be decoded): forget it.
    case gone
    /// A passing error (device locked before first unlock, Keychain busy): keep it, retry at the next start.
    case transient
}

/// Remembers the core's vault data key on this device so the node can unlock at launch without the passphrase (android
/// UnlockStore over AndroidKeyStore). One item per account, keyed by DID. Never logs key material.
protocol ShellKeyStore: AnyObject, Sendable {
    func exists(_ did: String) -> Bool
    func load(_ did: String) -> ShellKeyLoad
    /// Writes and reads back (S1: a key that cannot be opened again is no key). False on failure (nothing remembered).
    func save(_ did: String, _ secret: Data) -> Bool
    func clear(_ did: String)
}

/// The Keychain equivalent of the Android Keystore blob: a generic-password item per DID, readable after the first unlock
/// since boot (so a VoIP-push launch can open the vault), never synced or backed up off this device.
/// TODO(ios-device): verify on a device: item survival across app reinstall (Keychain items outlive the app, unlike Android's
/// files; a reinstall then finds a key for a vault that no longer exists, which the core answers with WrongPassphrase and we
/// clear), errSecInteractionNotAllowed before first unlock (mapped to .transient), and the access group once an extension
/// (notification service) needs it.
final class ShellKeychainStore: ShellKeyStore, @unchecked Sendable {
    private let service = "com.osvauld.tinline.unlock.v1"

    private func base(_ did: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: did]
    }

    func exists(_ did: String) -> Bool {
        var q = base(did)
        q[kSecReturnAttributes as String] = true
        return SecItemCopyMatching(q as CFDictionary, nil) == errSecSuccess
    }

    func load(_ did: String) -> ShellKeyLoad {
        var q = base(did)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        switch SecItemCopyMatching(q as CFDictionary, &out) {
        case errSecSuccess:
            guard let d = out as? Data, d.count == 32 else { return .gone }
            return .key(d)
        case errSecItemNotFound: return .gone
        case let status: NSLog("Tinline: keychain load failed: %d", status); return .transient
        }
    }

    func save(_ did: String, _ secret: Data) -> Bool {
        SecItemDelete(base(did) as CFDictionary)
        var add = base(did)
        add[kSecValueData as String] = secret
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let added = SecItemAdd(add as CFDictionary, nil)
        guard added == errSecSuccess else { NSLog("Tinline: keychain add failed: %d", added); return false }
        if case .key(var back) = load(did) { back.resetBytes(in: 0..<back.count); return true }
        NSLog("Tinline: keychain read-back failed (saved %d bytes)", secret.count)
        return false
    }

    func clear(_ did: String) { SecItemDelete(base(did) as CFDictionary) }
}

/// In-memory store for previews and tests.
final class ShellMemoryKeyStore: ShellKeyStore, @unchecked Sendable {
    private var items: [String: Data] = [:]
    private let lock = NSLock()
    func exists(_ did: String) -> Bool { lock.lock(); defer { lock.unlock() }; return items[did] != nil }
    func load(_ did: String) -> ShellKeyLoad { lock.lock(); defer { lock.unlock() }; return items[did].map { .key($0) } ?? .gone }
    func save(_ did: String, _ secret: Data) -> Bool { lock.lock(); items[did] = secret; lock.unlock(); return true }
    func clear(_ did: String) { lock.lock(); items[did] = nil; lock.unlock() }
}

// ---------------------------------------------------------------- screen lock check (android DeviceAuth.kt)

/// "Check it's you" with the phone's own screen lock, for accounts without a passphrase.
protocol ShellDeviceAuth {
    /// A passcode (or biometrics backed by one) is set. Recovery must not treat "no prompt" as authentication.
    var available: Bool { get }
    func ask(reason: String) async -> Bool
}

/// TODO(ios-device): check the prompt copy and the passcode fallback on a device (Face ID needs NSFaceIDUsageDescription).
struct ShellLocalAuth: ShellDeviceAuth {
    var available: Bool { LAContext().canEvaluatePolicy(.deviceOwnerAuthentication, error: nil) }
    func ask(reason: String) async -> Bool {
        (try? await LAContext().evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason)) ?? false
    }
}

// ---------------------------------------------------------------- permissions (android Perms.kt `Need`)

/// What the app needs from the system. iOS has no battery-optimisation or full-screen-intent switch: an incoming call rings
/// through CallKit, so only these two are asked.
enum ShellNeed: Hashable {
    case mic, notifications
}

protocol ShellPermissions {
    func missing() async -> [ShellNeed]
    /// Asks the system (a dialog the first time; afterwards only Settings can change it). True when granted.
    func request(_ need: ShellNeed) async -> Bool
    func openSettings()
}

/// TODO(ios-device): AVAudioApplication and UNUserNotificationCenter prompts need a device check (and the Info.plist
/// NSMicrophoneUsageDescription string).
struct ShellSystemPermissions: ShellPermissions {
    func missing() async -> [ShellNeed] {
        var out: [ShellNeed] = []
        if AVAudioApplication.shared.recordPermission != .granted { out.append(.mic) }
        let n = await UNUserNotificationCenter.current().notificationSettings()
        if n.authorizationStatus != .authorized && n.authorizationStatus != .provisional { out.append(.notifications) }
        return out
    }

    func request(_ need: ShellNeed) async -> Bool {
        switch need {
        case .mic:
            if AVAudioApplication.shared.recordPermission == .denied { openSettings(); return false }
            return await AVAudioApplication.requestRecordPermission()
        case .notifications:
            let c = UNUserNotificationCenter.current()
            if await c.notificationSettings().authorizationStatus == .denied { openSettings(); return false }
            return (try? await c.requestAuthorization(options: [.alert, .sound, .badge])) ?? false
        }
    }

    @MainActor func openSettingsNow() {
        if let u = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(u) }
    }
    func openSettings() { Task { @MainActor in openSettingsNow() } }
}

// ---------------------------------------------------------------- device-local preferences (android SharedPreferences)

/// Device-local settings the core does not hold: the last account, terms version, appearance, video and alerts.
/// Other areas read the call, alert and video keys through `ShellPrefs()` (UserDefaults, the same keys).
final class ShellPrefs: @unchecked Sendable {
    private let d: UserDefaults
    init(_ defaults: UserDefaults = .standard) { d = defaults }

    /// android `LegalStore.CURRENT`: bump to ask again.
    static let termsVersion = 1

    var lastAccount: String? {
        get { d.string(forKey: "accounts.did") }
        set { d.set(newValue, forKey: "accounts.did") }
    }
    var termsAccepted: Bool { d.integer(forKey: "legal.terms_version") >= Self.termsVersion }
    func acceptTerms() {
        d.set(Self.termsVersion, forKey: "legal.terms_version")
        d.set(Date().timeIntervalSince1970, forKey: "legal.terms_accepted_at")
    }
    /// nil follows the system; true/false is an explicit choice (Settings > Appearance, the sign-up switch).
    var appearanceDark: Bool? {
        get { d.object(forKey: "appearance.dark") as? Bool }
        set { if let newValue { d.set(newValue, forKey: "appearance.dark") } else { d.removeObject(forKey: "appearance.dark") } }
    }
    /// Offer and take video in calls; default on.
    var videoEnabled: Bool {
        get { d.object(forKey: "calls.video") as? Bool ?? true }
        set { d.set(newValue, forKey: "calls.video") }
    }
    /// "Sharper video": true = PREFER_HD, false = AUTO. Takes effect at the next camera start (Calls area reads it).
    var preferHd: Bool {
        get { d.string(forKey: "calls.video_quality") == "PREFER_HD" }
        set { d.set(newValue ? "PREFER_HD" : "AUTO", forKey: "calls.video_quality") }
    }
    /// The call sound set: "classic", "slack", "triad" (Calls area reads it).
    var callPalette: String {
        get { d.string(forKey: "calls.palette") ?? "classic" }
        set { d.set(newValue, forKey: "calls.palette") }
    }
    /// Settings > Notifications "Alert me about" and Settings > Sounds Quiet: "messages", "calls", "groups" default on,
    /// "quiet" default off.
    func alert(_ key: String) -> Bool { d.object(forKey: "alerts." + key) as? Bool ?? (key != "quiet") }
    func setAlert(_ key: String, _ on: Bool) { d.set(on, forKey: "alerts." + key) }
    /// When the Calls tab was last open (unix seconds): the red count is missed calls since then.
    var callsSeen: Int64 {
        get { Int64(d.integer(forKey: "chat_ui.calls_seen")) }
        set { d.set(Int(newValue), forKey: "chat_ui.calls_seen") }
    }
}

// ---------------------------------------------------------------- clipboard, links

enum ShellClipboard {
    /// Copies `text` local-only and expiring after 2 minutes (the iOS form of Android's EXTRA_IS_SENSITIVE: never Handoff).
    static func copySensitive(_ text: String) {
        UIPasteboard.general.setItems([[UIPasteboard.typeAutomatic: text]],
                                      options: [.localOnly: true, .expirationDate: Date().addingTimeInterval(120)])
    }
    static func text() -> String? { UIPasteboard.general.string }
}

enum ShellURL {
    static let terms = "https://tinline.osvauld.com/terms"
    static let privacy = "https://tinline.osvauld.com/privacy"
    static let site = "https://tinline.osvauld.com"
    static let source = "https://github.com/osvauld/tinline"

    @MainActor static func open(_ s: String) { if let u = URL(string: s) { UIApplication.shared.open(u) } }
}

// ---------------------------------------------------------------- network (android CoreService.netCb)

/// Tells the node about network changes (and whether the link is metered), like the Android service's network callback.
final class ShellNetworkWatch: @unchecked Sendable {
    private let monitor = NWPathMonitor()
    init(_ changed: @escaping (Bool) -> Void) {
        monitor.pathUpdateHandler = { path in changed(path.isExpensive || path.isConstrained) }
        monitor.start(queue: DispatchQueue(label: "p2p-net"))
    }
    deinit { monitor.cancel() }
}

// ---------------------------------------------------------------- QR

enum ShellQrImage {
    /// The QR for `text` in ink on white (Link.kt `qrBitmap(qr, 624, fg = 0xFF17201D)`), crisp at any size.
    static func make(_ text: String) -> UIImage? {
        let f = CIFilter.qrCodeGenerator()
        f.message = Data(text.utf8)
        f.correctionLevel = "M"
        guard let raw = f.outputImage else { return nil }
        let tint = CIFilter.falseColor()
        tint.inputImage = raw
        tint.color0 = CIColor(red: 0x17 / 255.0, green: 0x20 / 255.0, blue: 0x1D / 255.0)
        tint.color1 = CIColor(red: 1, green: 1, blue: 1)
        guard let out = tint.outputImage?.transformed(by: CGAffineTransform(scaleX: 12, y: 12)),
              let cg = CIContext().createCGImage(out, from: out.extent) else { return nil }
        return UIImage(cgImage: cg)
    }
}

/// The camera that reads a QR code (android QrCameraBox). One implementation per platform build; previews use the stub.
protocol ShellQrScanning {
    /// A view that shows the camera, calls `onText` with each decoded code and `onGranted` with the camera permission.
    func view(prompt: String, torch: Bool, onGranted: @escaping (Bool) -> Void, onText: @escaping (String) -> Void) -> AnyView
}

/// TODO(ios-device): the real scanner is an AVCaptureSession with an AVCaptureMetadataOutput (metadataObjectTypes [.qr]) in a
/// UIViewRepresentable, the torch through AVCaptureDevice.torchMode, and NSCameraUsageDescription in Info.plist. Not written
/// blind: this stub shows the prompt and nothing else, so the typed-code path is the way through on a simulator.
struct ShellQrScannerStub: ShellQrScanning {
    func view(prompt: String, torch: Bool, onGranted: @escaping (Bool) -> Void, onText: @escaping (String) -> Void) -> AnyView {
        AnyView(ZStack {
            Color(hex: 0x050807)
            Text(prompt).font(TinType.bodyM).foregroundStyle(Color(hex: 0x9DB0AA)).multilineTextAlignment(.center).padding(24)
        })
    }
}
