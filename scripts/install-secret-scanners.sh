#!/usr/bin/env bash
set -euo pipefail

GITLEAKS_VERSION=8.30.1
BETTERLEAKS_VERSION=1.10.1
KINGFISHER_VERSION=2.20.0

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <install-dir>" >&2
  exit 2
fi
dest=$1

case "$(uname -s)/$(uname -m)" in
  Linux/x86_64 | Linux/amd64)
    arch=x64
    gitleaks_sha=551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb
    betterleaks_sha=c174b42d7aa60caf58c0c3e555c0e4bed01fe6b0d99a412bce777b5af4ce913e
    kingfisher_sha=825a5e89cdf6c0d52e85bf029061f76ad6ba7cc6e55406c6eb9f68572ee6b774
    ;;
  Linux/aarch64 | Linux/arm64)
    arch=arm64
    gitleaks_sha=e4a487ee7ccd7d3a7f7ec08657610aa3606637dab924210b3aee62570fb4b080
    betterleaks_sha=62a63199f9f61a6b5e5a6f58f048493db20fbc8c770014f4d903a4d97886e1bf
    kingfisher_sha=245c34ee00caf24bffe9fed54a23bcb617fe95a0755ff881ef7d2c869f9d4ad7
    ;;
  *)
    echo "no pinned secret scanner assets for $(uname -s)/$(uname -m)" >&2
    exit 1
    ;;
esac

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$dest/bin"

fetch() {
  local name=$1 url=$2 sha=$3
  curl --fail --silent --show-error --location --retry 3 --output "$work/$name.tgz" "$url"
  if ! printf '%s  %s\n' "$sha" "$work/$name.tgz" | sha256sum --check --status; then
    echo "sha256 mismatch for $name ($url): refusing to install" >&2
    exit 1
  fi
  mkdir "$work/$name"
  tar --extract --gzip --file "$work/$name.tgz" --directory "$work/$name" "$name"
  install -m 0755 "$work/$name/$name" "$dest/bin/$name"
}

fetch gitleaks \
  "https://github.com/gitleaks/gitleaks/releases/download/v${GITLEAKS_VERSION}/gitleaks_${GITLEAKS_VERSION}_linux_${arch}.tar.gz" \
  "$gitleaks_sha"
fetch betterleaks \
  "https://github.com/betterleaks/betterleaks/releases/download/v${BETTERLEAKS_VERSION}/betterleaks_${BETTERLEAKS_VERSION}_linux_${arch}.tar.gz" \
  "$betterleaks_sha"
fetch kingfisher \
  "https://github.com/mongodb/kingfisher/releases/download/v${KINGFISHER_VERSION}/kingfisher-linux-${arch}.tgz" \
  "$kingfisher_sha"

if [ -n "${GITHUB_PATH:-}" ]; then
  printf '%s\n' "$dest/bin" >>"$GITHUB_PATH"
fi
echo "installed gitleaks $GITLEAKS_VERSION, betterleaks $BETTERLEAKS_VERSION, kingfisher $KINGFISHER_VERSION into $dest/bin"
