#!/bin/sh
# Installs the gate's dependency and workflow checks and the package's licence notices into /usr/local/bin, from
# their pinned releases' prebuilt binaries, each checked against a pinned checksum: the release's published one
# where it has one, else the sum of the release asset as first downloaded.
set -eu

DEST=/usr/local/bin

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# fetch <url> <sha256> <path of the binary in the archive>
fetch() {
    curl -sfL -o "$tmp/tool.tgz" "$1"
    echo "$2  $tmp/tool.tgz" | sha256sum -c -
    tar -xzf "$tmp/tool.tgz" -C "$tmp" "$3"
    mv "$tmp/$3" "$DEST/"
    rm "$tmp/tool.tgz"
}

fetch https://github.com/EmbarkStudios/cargo-deny/releases/download/0.20.2/cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz \
    9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f \
    cargo-deny-0.20.2-x86_64-unknown-linux-musl/cargo-deny
fetch https://github.com/EmbarkStudios/cargo-about/releases/download/0.9.3/cargo-about-0.9.3-x86_64-unknown-linux-musl.tar.gz \
    3ad1e94ea1e63d69d1961f4fb15ccbbe9575d906111f96d2cb4dce6df48f71b4 \
    cargo-about-0.9.3-x86_64-unknown-linux-musl/cargo-about
fetch https://github.com/Boshen/cargo-shear/releases/download/v1.14.0/cargo-shear-x86_64-unknown-linux-musl.tar.gz \
    9004df4eb031a456026c4326ae0d0ce545321527a67be90f4524c1be729dc40e \
    cargo-shear
fetch https://github.com/zizmorcore/zizmor/releases/download/v1.32.0/zizmor-x86_64-unknown-linux-gnu.tar.gz \
    bb110cc765eca9cd7efb2e284a956be5974c60027b0970f16397d86208a9957b \
    zizmor
fetch https://github.com/rhysd/actionlint/releases/download/v1.7.12/actionlint_1.7.12_linux_amd64.tar.gz \
    8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8 \
    actionlint
