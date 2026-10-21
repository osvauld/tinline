import SwiftUI

@main
struct TinlineApp: App {
    /// The one app state (android P2pApp); the Shell's `RootView` creates the node in it on first appearance.
    @StateObject private var app = AppEnv()

    var body: some Scene {
        WindowGroup {
            // RootView is the Shell's (Screens/Shell/ShellRoot.swift): the gates, then the home stack.
            RootView().environmentObject(app).tinTheme()
        }
    }
}
