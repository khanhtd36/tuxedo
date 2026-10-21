#!/bin/sh
# Writes the winget manifests for a published fork release into
# manifests/k/khanhtd36/tuxedo-khanhtd36/<version>, laid out as in
# microsoft/winget-pkgs, from the release's Windows zip + its .sha256
# sidecar. Later releases are submitted by the fork-release workflow's
# winget job; this is for the first one, or by hand.
#   scripts/winget-manifests.sh 2026.8.1-khanhtd36.4 [out-dir]
set -eu
version="${1:?version, such as 2026.8.1-khanhtd36.4}"
out="${2:-manifests}/k/khanhtd36/tuxedo-khanhtd36/$version"
repo="khanhtd36/tuxedo"
id="khanhtd36.tuxedo-khanhtd36"
tag="fork-v$version"
base="https://github.com/$repo/releases/download/$tag"
asset="tuxedo-${tag}-x86_64-pc-windows-msvc.zip"
schema="1.12.0"

x64="$(curl -fsSL "$base/$asset.sha256" | awk '{print toupper($1)}')"
[ -n "$x64" ] || { echo "no $asset.sha256 in $base" >&2; exit 1; }
date="$(curl -fsSL "https://api.github.com/repos/$repo/releases/tags/$tag" | grep -m1 '"published_at"' | sed -E 's/.*"([0-9]{4}-[0-9]{2}-[0-9]{2}).*/\1/')"

mkdir -p "$out"
cat > "$out/$id.yaml" <<EOF
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.version.$schema.schema.json

PackageIdentifier: $id
PackageVersion: $version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: $schema
EOF

cat > "$out/$id.installer.yaml" <<EOF
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.installer.$schema.schema.json

PackageIdentifier: $id
PackageVersion: $version
InstallerType: zip
NestedInstallerType: portable
NestedInstallerFiles:
- RelativeFilePath: tuxedo.exe
  PortableCommandAlias: tuxedo
Commands:
- tuxedo
ReleaseDate: $date
Installers:
- Architecture: x64
  InstallerUrl: $base/$asset
  InstallerSha256: $x64
ManifestType: installer
ManifestVersion: $schema
EOF

cat > "$out/$id.locale.en-US.yaml" <<EOF
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.defaultLocale.$schema.schema.json

PackageIdentifier: $id
PackageVersion: $version
PackageLocale: en-US
Publisher: khanhtd36
PublisherUrl: https://github.com/khanhtd36
PublisherSupportUrl: https://github.com/$repo/issues
Author: khanhtd36
PackageName: tuxedo-khanhtd36
PackageUrl: https://github.com/$repo
License: MIT
LicenseUrl: https://github.com/$repo/blob/main/LICENSE
Copyright: Copyright (c) 2026 Webstone Technologies Inc
CopyrightUrl: https://github.com/$repo/blob/main/LICENSE
ShortDescription: Fast, keyboard-driven terminal UI for todo.txt (khanhtd36's fork)
Description: tuxedo is a keyboard-driven terminal UI for todo.txt files. This is khanhtd36's fork of webstonehq/tuxedo, adding mouse support, self-update from the CLI and TUI, and other fork-only changes layered on top of upstream.
Moniker: tuxedo-khanhtd36
Tags:
- todo
- todotxt
- terminal
- tui
- productivity
ReleaseNotesUrl: https://github.com/$repo/releases/tag/$tag
ManifestType: defaultLocale
ManifestVersion: $schema
EOF
echo "$out"
