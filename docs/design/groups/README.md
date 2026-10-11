# Groups, blocking and address-change design

**Refinement study:** [Groups](../refined/groups.html), [Privacy & address rotation](../refined/privacy.html), and [Calls including group-call host loss](../refined/calls.html) are interactive, simulated design proposals. Targeted copy corrections below follow the later U1–U24 decisions; the original remote canvas has not been changed. [Review guide](../refined/README.md).

Copied from the "Tinline groups, blocking & rotation design" canvas
(https://claude.ai/artifact/KS9HUDfQzRFX9tpayTt76a), 2026-10-10. Same tokens and style as
`docs/design/Main.dc.html`. Each `*.dc.html` is one board (open in a browser); the paragraph under
each screen describes its behaviour. The canvas is the source of truth.

- `Main.dc.html` - Android: chats with groups, group chat, new group, group invite
- `AndroidGroupInfo.dc.html` - group info, member actions, flag dialog, hosts
- `AndroidRemoval.dc.html` - removed person's view, removal details, group's view, no host reachable
- `AndroidChatRequest.dc.html` - ask for a private chat, sent requests, incoming request, accepted
- `AndroidBlocking.dc.html` - block confirm, blocker's chat, blocked person's chat, blocked list
- `AndroidPrivacy.dc.html` - message/call switches, calling a message-only contact, address changes, lost contact
- `DesktopGroups.dc.html` - desktop group chat + members pane, Hosting settings, asked to host, contact panel

Decisions behind the screens: `endpoint-rotation-blocking:docs/design/rotation-open-decisions.md` section 0.
