#!/usr/bin/env bash
# Build the signed apt repository that GitHub Pages serves at
# https://dkjsiogu.github.io/cairn/apt.
#
# Usage: scripts/build-apt-repo.sh <site-dir> <deb>...
#
# The signing key must be in the gpg keyring ($GNUPGHOME or the default).
# Set APT_KEY to its fingerprint when the keyring holds more than one key.
# The result is checked against packaging/linux/apt/cairn-archive-keyring.gpg,
# the key every installed Cairn trusts.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
site=${1:?usage: build-apt-repo.sh <site-dir> <deb>...}
shift
if [ $# -eq 0 ]; then
  echo "no .deb given" >&2
  exit 1
fi
site=$(mkdir -p "$site" && cd "$site" && pwd)
repo="$site/apt"
keyring="$root/packaging/linux/apt/cairn-archive-keyring.gpg"

rm -rf "$repo"
mkdir -p "$repo/pool/main/c/cairn" "$repo/dists/stable/main/binary-amd64"
cp "$@" "$repo/pool/main/c/cairn/"

cd "$repo"
apt-ftparchive packages pool/main > dists/stable/main/binary-amd64/Packages
gzip -9nk dists/stable/main/binary-amd64/Packages

release=$(mktemp)
apt-ftparchive \
  -o APT::FTPArchive::Release::Origin=Cairn \
  -o APT::FTPArchive::Release::Label=Cairn \
  -o APT::FTPArchive::Release::Suite=stable \
  -o APT::FTPArchive::Release::Codename=stable \
  -o APT::FTPArchive::Release::Architectures=amd64 \
  -o APT::FTPArchive::Release::Components=main \
  -o "APT::FTPArchive::Release::Description=Cairn Minecraft launcher" \
  release dists/stable > "$release"
mv "$release" dists/stable/Release
chmod 644 dists/stable/Release

key=()
if [ -n "${APT_KEY:-}" ]; then key=(--local-user "$APT_KEY"); fi
gpg --batch --yes "${key[@]}" --digest-algo SHA512 --clearsign \
  -o dists/stable/InRelease dists/stable/Release
gpg --batch --yes "${key[@]}" --digest-algo SHA512 --armor --detach-sign \
  -o dists/stable/Release.gpg dists/stable/Release

# Signed by the key installed Cairn trusts, or nobody can update.
gpgv --keyring "$keyring" dists/stable/InRelease
gpgv --keyring "$keyring" dists/stable/Release.gpg dists/stable/Release

install -m644 "$keyring" "$repo/cairn-archive-keyring.gpg"
install -m644 "$root/packaging/linux/apt/cairn.sources" "$repo/cairn.sources"
install -m644 "$root/packaging/linux/apt/index.html" "$site/index.html"
touch "$site/.nojekyll"

echo
echo "apt repository: $repo"
grep -E '^(Package|Version|Filename):' dists/stable/main/binary-amd64/Packages
