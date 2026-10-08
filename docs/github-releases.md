# GitHub release builds

`.github/workflows/release.yml` builds Linux x86_64 tarballs, a Debian package,
and a signed universal Android release APK (arm64-v8a + x86_64). Tag builds
create a **draft** GitHub release only after both platform jobs succeed. Manual
runs produce Actions artifacts without creating a GitHub release.

## One-time setup

1. Push the workflow to GitHub (`osvauld/tinline`). Enable Actions if needed.
2. Create a GitHub environment named `release` under Settings → Environments.
   Add required reviewers if available on your plan, and restrict it to trusted
   release tags and the branch used for manual test runs.
3. Add these environment secrets:
   - `ANDROID_KEYSTORE_BASE64`: base64-encoded release keystore
   - `ANDROID_STORE_PASSWORD`
   - `ANDROID_KEY_ALIAS`
   - `ANDROID_KEY_PASSWORD`
4. Before merging, push `release/linux-android-packaging` to trigger both builds.
   Allow that trusted branch in the `release` environment deployment rules and
   approve the job if required. Branch builds upload artifacts only, not releases.
   After merging into the default branch, manual Run workflow is also available.

Do not paste secrets into chat, commit them, or include signing keys in artifacts.
Base64 is encoding, not encryption. Keep an encrypted backup of the signing key
and passwords outside GitHub. CI fails instead of falling back to debug signing.

If you already have a signing key for distributed APKs, reuse it. Android requires
the same certificate for updates. If Play App Signing is enabled, an upload key
is not necessarily the app signing key: APKs signed with it cannot update a
Play-installed app signed with another certificate.

If this is the first direct APK distribution, generate a dedicated release key
locally using `keytool` (it prompts for passwords):

```sh
keytool -genkeypair -keystore tinline-release.jks -alias tinline \
  -keyalg RSA -keysize 4096 -validity 10000
# Encode locally and put the result directly into the GitHub secret UI.
base64 -w0 tinline-release.jks
```

## Release

Set workspace version in `Cargo.toml`, commit, then tag that commit:

```sh
git tag v0.1.0
git push origin v0.1.0
```

The tag must match the workspace version. Android versionName is taken from that
version, and versionCode uses the commit count. Keep linear release ancestry and
check the code exceeds all previously distributed/Play-uploaded versions; do not
rewrite release history.

Download artifacts from the manual run or draft release. Test desktop install,
launch, chat and calls on a clean Ubuntu 24.04 VM, and install the APK on a real
phone. The workflow installs the Debian package on the build runner, but this is
**not** a clean-system or GUI functional test. Older Debian/Ubuntu versions are
not promised compatible; build against an older baseline separately if needed.
The Ubuntu-built tarball must also be tested on current Arch before publishing
`tinline-bin` to AUR. Resolve all dynamic dependencies and replace the AUR checksum
placeholder with a real checksum, then regenerate `.SRCINFO`.

Publishing the draft makes it visible on GitHub Releases. This workflow does not
publish to R2, sign APT metadata, submit to AUR, or upload to Play. Those are separate
steps after release verification. No Cloudflare/GPG/AUR secrets are needed yet.
