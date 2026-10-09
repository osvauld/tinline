# Linux packaging

First channels:

- Debian/Ubuntu APT repo at `https://repo.osvauld.com/apt`
- Arch AUR package `tinline-bin`
- Release tarballs at `https://repo.osvauld.com/releases/vX.Y.Z/`

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
sudo apt install dpkg-dev apt-utils
GPG_KEY_ID=<repo-signing-key> ./scripts/build_apt_repo.sh
```

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
