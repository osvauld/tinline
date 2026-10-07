# Identity at rest (passphrase vault)

`profile.json` in the app data dir holds the identity. Format (version 2):

```json
{
  "version": 2,
  "name": "alice",
  "did": "did:key:...",
  "device_public": [32 numbers],
  "vault": { "salt": "b64", "m": 65536, "t": 3, "p": 4, "wrapped_dek": "b64", "sealed": "b64" }
}
```

`name`, `did` and `device_public` are in the clear so a lock screen can show who this is.
Everything secret is in `vault` (all bytes standard base64):

- `sealed` = AES-256-GCM (`nonce || ct || tag`) of the JSON `{"mnemonic", "device_secret"}` under a random
  32-byte data key (DEK).
- `wrapped_dek` = the DEK, AES-256-GCM under KEK = Argon2id(passphrase, `salt`, m KiB, t, p) -> 32 bytes.
  Defaults are osvauld's keystore parameters: m=65536, t=3, p=4, 16-byte salt. A failed tag is reported as
  `WrongPassphrase`. Params outside m 64-256 MiB, t 3-10, p 1-8 or a salt outside 16-64 bytes (a tampered or downgraded
  file) are refused.
- Changing the passphrase rewraps the DEK (new salt) and leaves `sealed` and the DEK unchanged.

The core never writes the DEK anywhere. `Node::unlock_key()` hands it to the platform, which may keep it
wrapped by hardware (Android Keystore) so the always-on service can `unlock_with_key` after a reboot without
the passphrase. If that key stops opening the vault, `unlock_with_key` returns `WrongPassphrase` and the
platform must delete its copy.

Legacy installs have the old shape (`{"mnemonic","name","device_secret"}`, no `version`). They load unlocked
so calls keep working but report `LockState::NeedsPassphrase`; `set_passphrase(None, new)` seals them and
atomically replaces `profile.json` without the clear secrets.

## Threat model

Protects the recovery phrase and keys against someone who copies the app's files or a backup: they get
ciphertext, only as strong as the passphrase (Argon2id slows guessing; use a long one). It does not protect
against malware running as the app or a rooted device while unlocked. With a platform-remembered key, a stolen
phone that can unlock itself can still receive and place calls, but cannot reveal the recovery phrase
(`recovery_phrase(passphrase)` re-derives from the passphrase) nor change the passphrase without it.
Minimum passphrase length is 8 characters (`Error::WeakPassphrase`).

## Files

The data dir is created `0700` and every file written is `0600` (existing installs are fixed up when
opened); leftover `*.tmp` files from an interrupted write are deleted on open. `Profile` (mnemonic and
device secret in memory) is zeroized on drop.
