# Publishing releases to R2

`Publish release to R2` runs when a stable GitHub release is **published**, not
when its draft is created or a branch builds. It can also be run manually for an
existing published tag. Merge the workflow into the default branch before relying
on release events or manual dispatch.

## GitHub configuration

In Settings → Environments → `release`, configure:

Secrets:

- `R2_ACCESS_KEY_ID`
- `R2_SECRET_ACCESS_KEY` (scope credentials to this bucket)
- `APT_GPG_PRIVATE_KEY` (ASCII-armored private signing key, not Base64)
- `APT_GPG_PASSPHRASE`

Environment **variables** (not secrets):

- `R2_BUCKET`: `tinline`
- `R2_ENDPOINT`: `https://<account-id>.r2.cloudflarestorage.com`
- `APT_GPG_KEY_ID`: full signing-key fingerprint

R2 credentials configured as secrets for bucket/endpoint must be moved to
variables or the workflow adapted. Android secrets remain unchanged. Enable
required reviewers if supported, and allow only reviewed release branches/tags.
R2 hosting is public: APKs and tarballs become downloadable immediately on upload.

## Create an APT key locally

Use a dedicated package-repository key, separate from Android signing. Keep an
encrypted offline backup and never commit/paste private material into chat.

```sh
gpg --quick-generate-key 'Tinline package repository <contact@osvauld.com>' rsa4096 sign 2y
gpg --list-secret-keys --keyid-format long
# Copy the full fingerprint into APT_GPG_KEY_ID.
# Export locally to a file, then put its entire contents in APT_GPG_PRIVATE_KEY.
gpg --armor --export-secret-keys <FULL_FINGERPRINT> > /tmp/tinline-apt-private.asc
```

Add the key's password as `APT_GPG_PASSPHRASE`. Delete the temporary private-key
export after transferring it. The workflow exports the corresponding public key
as `https://repo.osvauld.com/tinline.gpg` automatically.

## Publication

1. Complete builds and test installation, chat, calls and Android updates.
2. Tag the approved commit; let Release builds create its draft with all assets.
3. Publish the draft on GitHub, then approve the R2 job if required.
4. Verify the public key, APK, tarball and APT InRelease URLs over HTTPS.
5. Test `apt update` and installation on a clean Ubuntu 24.04 machine.

Bucket layout:

```text
tinline.gpg
releases/v0.1.0/tinline-v0.1.0.apk
releases/v0.1.0/tinline-v0.1.0-x86_64-linux.tar.gz
releases/v0.1.0/SHA256SUMS
apt/pool/main/t/tinline/tinline_0.1.0_amd64.deb
apt/dists/stable/InRelease
```

Older pool packages and by-hash indices are retained. Existing versioned artifacts
cannot be replaced with different contents. Package/index objects upload before
signed metadata; InRelease is uploaded last. Never use `sync --delete`. Avoid
Cloudflare cache rules that override the metadata's no-cache headers. The tarball
is Ubuntu-built and still needs Arch compatibility testing before AUR submission.

APT metadata expires after 30 days to limit replay attacks. Until scheduled
renewal is implemented, **rerun publishing for the latest published tag at least
monthly**, even if there is no new release. Monitor key expiry too. Do not rotate
the public signing key without planning how existing clients receive the new key.

Public downloads do not make an app self-update: APT handles Debian updates,
AUR publishing is separate, and direct APK users must download/install updates
manually unless an in-app updater is added.
