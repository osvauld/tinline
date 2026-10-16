# Linux packaging

First channels:

- Debian/Ubuntu APT repo at `https://repo.osvauld.com/apt`
- Arch AUR package `tinline-bin`
- Portable AppImages and release tarballs at `https://repo.osvauld.com/releases/vX.Y.Z/`

## Verification status

Packaging is scaffolding, not a verified release. Build tarballs natively on the
intended distro; an Arch build is not automatically compatible with Debian/Ubuntu.
The local development host is Arch and lacks `dpkg-deb`/`dpkg-shlibdeps`.
Debian builds must run on Debian/Ubuntu with `dpkg-dev` installed; library
requirements are calculated using `dpkg-shlibdeps`. Test on the oldest supported
distro before claiming compatibility. AUR runtime dependencies also need checking
against the final tarball (including versioned Abseil dependencies).

Builds use `target/distribution` and `--locked`, without `test-hooks`, and queue
through `scripts/buildlock.py`. Build output is ignored under `dist/`.

Local build:

```sh
./scripts/package_linux_tarball.sh
./scripts/package_deb.sh
./scripts/package_appimage.sh
sudo apt install dpkg-dev apt-utils
GPG_KEY_ID=<repo-signing-key> ./scripts/build_apt_repo.sh
```

## AppImage

`package_appimage.sh` builds natively on x86_64 or aarch64, downloads a pinned,
SHA-256-verified linuxdeploy, and emits
`dist/releases/vX.Y.Z/tinline-X.Y.Z-linux-<arch>.AppImage` plus its checksum.
It bundles linked libraries and the X11/Wayland libraries loaded by winit;
libaom remains statically linked. Install the runtime library packages listed in
`.github/workflows/release.yml` on the build host. Packaging uses extract-and-run
and does not require FUSE.

```sh
chmod +x tinline-X.Y.Z-linux-x86_64.AppImage
./tinline-X.Y.Z-linux-x86_64.AppImage
# Without FUSE:
./tinline-X.Y.Z-linux-x86_64.AppImage --appimage-extract-and-run
```

This is not a sandbox or an automatic updater. The host still provides glibc,
graphics drivers/loaders, audio services, desktop portals and a Secret Service
keyring (when using remembered credentials). Ubuntu 24.04 is the CI build baseline;
an AppImage does not make newer glibc binaries work on older systems. Validate
launch, file dialogs, keyring, audio/video calls, X11 and Wayland on clean systems
before publishing. CI extraction/dependency checks are not a GUI smoke test.

Upload to Cloudflare R2:

```sh
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
export R2_BUCKET=osvauld-packages
export R2_ENDPOINT=https://<account-id>.r2.cloudflarestorage.com
./scripts/upload_repo_r2.sh
```

APT install target:

```sh
curl -fsSL https://repo.osvauld.com/tinline.gpg \
  | sudo tee /usr/share/keyrings/tinline.gpg >/dev/null

echo "deb [signed-by=/usr/share/keyrings/tinline.gpg] https://repo.osvauld.com/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/tinline.list

sudo apt update
sudo apt install tinline
```

Before publishing AUR, replace `SKIP` with the real SHA-256 from the tarball and regenerate `.SRCINFO` with `makepkg --printsrcinfo > .SRCINFO`.
