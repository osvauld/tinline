# Tinline privacy policy

_Last updated: 8 October 2026_

Tinline is a calling app made by Osvauld ("we"). It connects your device directly to the device of
the person you call. This policy explains what that means for your data. Short version: we do not
collect, store, sell or share your personal data, and the app contains no ads, analytics or
tracking code.

## What stays on your device

- **Your account.** Tinline creates a cryptographic identity on your device. Its secret keys never
  leave the device. They are stored encrypted, protected by the Android Keystore (or your
  computer's keyring) and, if you set one, your passphrase.
- **Your name, contacts, call history and settings.** These are stored only on your device,
  encrypted. We have no copy and no way to read them.
- **Your recovery phrase.** It is shown to you once and is not stored anywhere else. Anyone who has
  it can restore your account, so keep it private.

## What you share with people you add

When you add a contact, the two devices exchange your display names and public keys (device
identifiers). Your contact sees the name you chose. Nothing is shared with anyone you have not
added.

## Calls

- Calls are end-to-end encrypted between the two devices. Nobody in between, including us, can hear
  them.
- Tinline tries to connect the two devices directly. When a direct connection is not possible
  (for example behind some routers or mobile networks), the encrypted call is passed through a
  **relay server**. Today Tinline uses the public relays run by number 0, Inc. (the makers of the
  open-source iroh networking library). A relay can see the IP addresses of the two devices, a
  random device identifier, and the timing and amount of encrypted traffic. It cannot see who you
  are, your contacts or the content of calls.
- To let your contacts find your device, Tinline publishes your device identifier with its current
  relay address and network addresses to a discovery service, also run by number 0, Inc. This
  record contains no name, phone number or contact list.

## Permissions

- **Microphone:** to send your voice during a call. It is used only during calls.
- **Camera:** to scan a contact's QR code. Images are not saved or sent.
- **Notifications and full-screen notifications:** to ring when someone calls, including on the
  lock screen.
- **Run in the background, start at boot, ignore battery optimisation:** so the app can keep its
  connection open and receive calls when it is not on screen, like a phone line.

## What we do not do

- No account registration, phone number or email is required.
- No ads, analytics, crash reporting or tracking SDKs.
- We do not sell or share data, because we do not have it.

## Children

Tinline is not directed at children under 13.

## Deleting your data

Uninstalling Tinline deletes everything it stored on the device. Because we hold no copy, there is
nothing for us to delete on our side. If you keep your recovery phrase, you can restore your
identity later.

## Changes

We will update this page when Tinline changes what it does, for example when chat is added, and
change the date above.

## Contact

Osvauld — osvauld@gmail.com — https://tinline.osvauld.com
