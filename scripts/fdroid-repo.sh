#!/usr/bin/env bash
# Build the thencloud F-Droid repo (https://docs.kkaii.xyz/fdroid/repo) from
# the Android APKs of the latest GitHub releases.
#
#   scripts/fdroid-repo.sh OUT_DIR [APK...]
#
# With no APKs given, downloads the latest $FDROID_RELEASES (3) releases'
# `thencloud-vX.Y.Z-android.apk` with `gh`. Needs fdroidserver, and the
# repo's index key: FDROID_KEYSTORE (base64 PKCS#12) and FDROID_KEYSTORE_PASS.
# The APKs aren't changed: they keep the release workflow's signature, and
# the index only lists them, signed with its own key.
set -euo pipefail
out=$1
shift
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cp "$root/fdroid/config.yml" "$work/"
cp -r "$root/fdroid/metadata" "$work/"
cp "$root/crates/thencloud-app/icons/128x128@2x.png" "$work/icon.png"
mkdir -p "$work/metadata/org.thencloud.client/en-US"
cp "$root/crates/thencloud-app/icons/icon.png" "$work/metadata/org.thencloud.client/en-US/icon.png"
printf '%s' "$FDROID_KEYSTORE" | base64 -d > "$work/keystore.p12"
chmod 600 "$work/config.yml" "$work/keystore.p12"
mkdir -p "$work/repo"

if [ $# -gt 0 ]; then
  cp "$@" "$work/repo/"
else
  for tag in $(gh release list --repo 0x00ACAB/thencloud --exclude-drafts --exclude-pre-releases \
    --limit "${FDROID_RELEASES:-3}" --json tagName --jq '.[].tagName'); do
    gh release download "$tag" --repo 0x00ACAB/thencloud --pattern '*-android.apk' --dir "$work/repo" || true
  done
fi
ls "$work"/repo/*.apk >/dev/null

(cd "$work" && fdroid update --use-date-from-apk --pretty)
mkdir -p "$out"
rm -rf "$out/repo"
cp -r "$work/repo" "$out/repo"
echo "F-Droid repo in $out/repo:"
python3 - "$out/repo/index-v2.json" <<'PY'
import json, sys
idx = json.load(open(sys.argv[1]))
for pkg, p in idx["packages"].items():
    for v in p["versions"].values():
        m = v["manifest"]
        print(f"  {pkg} {m['versionName']} ({m['versionCode']})")
PY
