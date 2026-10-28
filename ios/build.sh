#!/usr/bin/env bash
# Builds Horizon for iOS on a Mac with Xcode: target/ios/Horizon.app and Horizon.ipa.
#
#   ios/build.sh [options]
#
#   --sim                  for the iOS Simulator on Apple Silicon (aarch64-apple-ios-sim)
#   --debug                a debug build (much slower in game; for debugging only)
#   --sign IDENTITY        sign with this certificate ("Apple Development: Name (TEAMID)");
#   --profile FILE         ...and this provisioning profile (.mobileprovision) for the device
#   --bundle-id ID         the app's bundle id (default io.github.worrcc.horizon, or
#                          HORIZON_IOS_BUNDLE_ID); a signed build must match the profile's
#   --resources DIR        put prepared resources (a horizon-prepare output folder) inside the
#                          app. Built from your own copy of Minecraft: for your devices only,
#                          never share the .app or .ipa then
#
# Without --sign the app is ad-hoc signed: install the .ipa with AltStore, SideStore or
# Sideloadly, which sign it with your Apple ID. ios/README.md has the whole story.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
target=aarch64-apple-ios
platform=iPhoneOS
profile_dir=release
cargo_profile=(--release)
identity=""
provision=""
bundle_id="${HORIZON_IOS_BUNDLE_ID:-io.github.worrcc.horizon}"
resources=""

while [ $# -gt 0 ]; do
    case "$1" in
        --sim) target=aarch64-apple-ios-sim; platform=iPhoneSimulator ;;
        --debug) profile_dir=debug; cargo_profile=() ;;
        --sign) identity="$2"; shift ;;
        --profile) provision="$2"; shift ;;
        --bundle-id) bundle_id="$2"; shift ;;
        --resources) resources="$2"; shift ;;
        -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
        *) echo "unknown option $1 (see --help)" >&2; exit 2 ;;
    esac
    shift
done

if [ "$(uname)" != Darwin ] || ! command -v xcrun >/dev/null; then
    echo "ios/build.sh needs macOS with Xcode (xcode-select --install is not enough)." >&2
    echo "On other systems, ios/scripts/check-ios.sh type-checks the iOS build." >&2
    exit 1
fi
if [ -n "$identity" ] && [ -z "$provision" ]; then
    echo "--sign needs --profile: a device only runs apps with an embedded profile" >&2
    exit 2
fi
if [ -n "$resources" ] && [ ! -f "$resources/horizon-resources.json" ]; then
    echo "$resources holds no prepared resources (no horizon-resources.json)" >&2
    exit 2
fi

export IPHONEOS_DEPLOYMENT_TARGET=15.0
rustup target add "$target" >/dev/null
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/launcher/Cargo.toml" | head -1)"

echo "Building horizon-client for $target ($profile_dir)..."
(cd "$root" && cargo build "${cargo_profile[@]}" --target "$target" -p horizon-client \
    --bin horizon-client)
binary="${CARGO_TARGET_DIR:-$root/target}/$target/$profile_dir/horizon-client"

out="${CARGO_TARGET_DIR:-$root/target}/ios"
app="$out/Horizon.app"
rm -rf "$app" "$out/Payload" "$out/Horizon.ipa"
mkdir -p "$app"
cp "$binary" "$app/horizon-client"
sed -e "s/@VERSION@/$version/g" -e "s/@BUNDLE_ID@/$bundle_id/g" -e "s/@PLATFORM@/$platform/g" \
    "$root/ios/App/Info.plist" > "$app/Info.plist"
plutil -lint "$app/Info.plist" >/dev/null
cp "$root"/ios/App/Icons/AppIcon*@*.png "$app/"
printf 'APPL????' > "$app/PkgInfo"
if [ -n "$resources" ]; then
    echo "Copying the prepared resources into the app (for your own devices only)..."
    cp -R "$resources" "$app/resources"
fi

if [ -n "$identity" ]; then
    cp "$provision" "$app/embedded.mobileprovision"
    entitlements="$out/entitlements.plist"
    security cms -D -i "$provision" > "$out/profile.plist"
    /usr/libexec/PlistBuddy -x -c 'Print :Entitlements' "$out/profile.plist" > "$entitlements"
    codesign --force --timestamp=none --sign "$identity" --entitlements "$entitlements" "$app"
else
    codesign --force --sign - "$app"
fi

mkdir -p "$out/Payload"
cp -R "$app" "$out/Payload/"
(cd "$out" && zip -qry Horizon.ipa Payload)
rm -rf "$out/Payload"

echo
echo "Built $app"
echo "  and $out/Horizon.ipa"
if [ "$platform" = iPhoneSimulator ]; then
    echo "Install: xcrun simctl install booted '$app' && xcrun simctl launch booted $bundle_id"
elif [ -n "$identity" ]; then
    echo "Install: xcrun devicectl device install app --device <device id> '$app'"
    echo "         (xcrun devicectl list devices shows the id)"
else
    echo "Install the .ipa with AltStore, SideStore or Sideloadly (ios/README.md)."
fi
