import SwiftUI

/// The playground scrim: rgb(10 16 14 / .5).
let tinScrim = Color(.sRGB, red: 10 / 255, green: 16 / 255, blue: 14 / 255, opacity: 0.5)

/// chat/Layer.svelte, variant `dialog` (or `sheet`): a bottom sheet over the whole app (android PgLayer.kt).
/// Scrim rgb(10 16 14 / .5); the sheet is full width up to 520, surface-raised, CSS radius 24 on top (the playground does not
/// smooth sheets), the sheet shadow, a 28 x 4 notch (8 above, 2 below), at most the frame height minus 32.
/// In (transitions.ts layerIn): 200 ms cubicOut, the sheet rises 24 pt while its opacity goes .4 -> 1, the scrim fades in.
/// Out (layerOut): 140 ms cubicIn, sinks 24 pt and fades to 0. Reduced motion: instant.
/// A tap on the scrim closes (as does the VoiceOver escape gesture). The binding going nil/false closes it too; the last value stays
/// drawn while it animates out.
///
/// It is presented with `fullScreenCover` (a clear background, its own transition switched off) so it covers the tab bar and
/// navigation bar like the Android dialog window does.
// TODO(ios-device): confirm the cover's own slide stays off on both present and dismiss on iOS 17 and 18, and the keyboard lift.
extension View {
    func tinLayer<Item, Content: View>(item: Binding<Item?>, label: String, @ViewBuilder content: @escaping (Item) -> Content) -> some View {
        modifier(TinLayerModifier(item: item, label: label, sheet: content))
    }

    /// Convenience for a boolean-driven layer.
    func tinLayer<Content: View>(isPresented: Binding<Bool>, label: String, @ViewBuilder content: @escaping () -> Content) -> some View {
        modifier(TinLayerModifier<Void, Content>(
            item: Binding(get: { isPresented.wrappedValue ? () : nil }, set: { isPresented.wrappedValue = $0 != nil }),
            label: label, sheet: { _ in content() }))
    }
}

private final class LayerLast<T> { var value: T? }

private struct TinLayerModifier<Item, Sheet: View>: ViewModifier {
    @Binding var item: Item?
    let label: String
    let sheet: (Item) -> Sheet
    @State private var covered = false
    @State private var last = LayerLast<Item>()

    func body(content: Content) -> some View {
        if let it = item { last.value = it }
        let open = item != nil
        return content
            .fullScreenCover(isPresented: $covered) {
                TinLayerHost(open: open, label: label, onClose: { item = nil }, onGone: {
                    // A new value may have arrived while the old one was sinking.
                    if item == nil { setCovered(false) }
                }) {
                    if let v = last.value { sheet(v) }
                }
                .presentationBackground(.clear)
            }
            .onChange(of: open, initial: true) { _, now in if now { setCovered(true) } }
    }

    private func setCovered(_ on: Bool) {
        var t = Transaction()
        t.disablesAnimations = true
        withTransaction(t) { covered = on }
    }
}

private struct HeightKey: PreferenceKey {
    static let defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}

private struct TinLayerHost<Content: View>: View {
    let open: Bool
    let label: String
    let onClose: () -> Void
    let onGone: () -> Void
    @ViewBuilder let content: () -> Content

    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    @State private var t = 0.0
    @State private var contentH: CGFloat = 0

    private let shape = UnevenRoundedRectangle(topLeadingRadius: 24, bottomLeadingRadius: 0, bottomTrailingRadius: 0, topTrailingRadius: 24, style: .circular)

    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .bottom) {
                tinScrim.opacity(t).ignoresSafeArea().contentShape(Rectangle())
                    .onTapGesture { if open { onClose() } }
                    .accessibilityHidden(true)
                // The sheet's own padding clears the home indicator (the background runs under it); the keyboard lifts the whole sheet.
                ScrollView {
                    VStack(spacing: 0) {
                        Capsule().fill(tin.ln).frame(width: 28, height: 4).padding(.top, 8).padding(.bottom, 2)
                        content()
                    }
                    .frame(maxWidth: .infinity)
                    .background(GeometryReader { Color.clear.preference(key: HeightKey.self, value: $0.size.height) })
                }
                .scrollBounceBehavior(.basedOnSize)
                .onPreferenceChange(HeightKey.self) { contentH = $0 }
                .frame(height: contentH > 0 ? min(contentH, max(0, geo.size.height - 32)) : nil)
                .frame(maxWidth: 520)
                .clipShape(shape)
                .background(shape.fill(tin.sfRaised).ignoresSafeArea(.container, edges: .bottom).tinDepth(.sheet, shape))
                .offset(y: (1 - t) * 24)
                .opacity(open ? 0.4 + 0.6 * t : t)
                .accessibilityElement(children: .contain)
                .accessibilityLabel(label)
                .accessibilityAddTraits(.isModal)
                .accessibilityAction(.escape) { if open { onClose() } }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
        }
        .onAppear { run(open) }
        .onChange(of: open) { _, now in run(now) }
    }

    private func run(_ opening: Bool) {
        if opening {
            if reduced { t = 1 } else { withAnimation(.tinCubicOut(TinMotion.enter)) { t = 1 } }
        } else if reduced {
            t = 0; onGone()
        } else {
            withAnimation(.tinCubicIn(TinMotion.exit), completionCriteria: .logicallyComplete) { t = 0 } completion: { onGone() }
        }
    }
}

extension View {
    /// Negative horizontal margin (CSS `margin-inline: -n`), used by lists that bleed into a dialog's padding.
    func tinBleedX(_ n: CGFloat) -> some View { padding(.horizontal, -n) }
}
