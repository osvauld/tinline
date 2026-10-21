import SwiftUI

@main
struct TinlineApp: App {
    var body: some Scene {
        WindowGroup {
            RootView().tinTheme()
        }
    }
}

/// Placeholder shell until the navigation layer lands (docs/design/SCREENS-HANDOFF.md).
struct RootView: View {
    @Environment(\.tin) private var tin
    var body: some View {
        ZStack { tin.bg.ignoresSafeArea(); Text("Tinline").font(TinType.display).foregroundStyle(tin.ink) }
    }
}
