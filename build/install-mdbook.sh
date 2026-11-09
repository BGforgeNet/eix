#!/bin/sh
# Installs the pinned mdBook release into a directory (default /usr/local/bin), checked against the release's
# published checksum. The build image and the Pages workflow both run this, so the version has one home.
set -eu

VERSION=0.5.4
SHA256=5222beabd3e37dc5be0d18ff99b79058469354db5c220153a1b92db5ba12be89

dest=${1:-/usr/local/bin}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

curl -sfL -o "$tmp/mdbook.tgz" \
    "https://github.com/rust-lang/mdBook/releases/download/v${VERSION}/mdbook-v${VERSION}-x86_64-unknown-linux-musl.tar.gz"
echo "${SHA256}  $tmp/mdbook.tgz" | sha256sum -c -
mkdir -p "$dest"
tar -xzf "$tmp/mdbook.tgz" -C "$dest" mdbook
