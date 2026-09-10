#!/bin/zsh
set -euo pipefail

SCRIPT_DIR="${0:A:h}"
PROJECT_DIR="${SCRIPT_DIR:h:h}"
DIST_DIR="$PROJECT_DIR/dist"
APP_DIR="$DIST_DIR/HiRodrop.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"
PLUGINS_DIR="$CONTENTS_DIR/PlugIns"
SHARE_SOURCE_DIR="$SCRIPT_DIR/share-extension"
SHARE_APP_DIR="$PLUGINS_DIR/HiRodropShare.appex"
SHARE_CONTENTS_DIR="$SHARE_APP_DIR/Contents"
SHARE_MACOS_DIR="$SHARE_CONTENTS_DIR/MacOS"
SHARE_RESOURCES_DIR="$SHARE_CONTENTS_DIR/Resources"
APP_ICON="$PROJECT_DIR/assets/hirodrop-icon.icns"
CARGO_ROOT="$(cd "$(dirname "$(command -v cargo)")/.." && pwd -P)"
RUST_ROOT="$(rustc --print sysroot)"
ARM_TARGET="aarch64-apple-darwin"
INTEL_TARGET="x86_64-apple-darwin"
SIGN_IDENTITY="${HIRODROP_SIGN_IDENTITY:--}"

cd "$PROJECT_DIR"
for TARGET in "$ARM_TARGET" "$INTEL_TARGET"; do
    if ! rustup target list --installed | grep -qx "$TARGET"; then
        print -u2 "Missing Rust target: $TARGET"
        print -u2 "Install it with: rustup target add $TARGET"
        exit 1
    fi
    MACOSX_DEPLOYMENT_TARGET=12.0 \
        RUSTFLAGS="--remap-path-prefix=$PROJECT_DIR=/build/HiRodrop --remap-path-prefix=$CARGO_ROOT=/build/cargo --remap-path-prefix=$RUST_ROOT=/build/rust" \
        cargo build --release --target "$TARGET" --bin hirodrop-gui
done

rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR" "$SHARE_MACOS_DIR" "$SHARE_RESOURCES_DIR"
xcrun lipo -create \
    "$PROJECT_DIR/target/$ARM_TARGET/release/hirodrop-gui" \
    "$PROJECT_DIR/target/$INTEL_TARGET/release/hirodrop-gui" \
    -output "$MACOS_DIR/hirodrop-gui"
cp "$SCRIPT_DIR/Info.plist" "$CONTENTS_DIR/Info.plist"
cp -R "$SCRIPT_DIR/localizations/." "$RESOURCES_DIR/"

SHARE_BUILD_DIR="$(mktemp -d)"
for ARCH in arm64 x86_64; do
    xcrun clang \
        -arch "$ARCH" \
        -mmacosx-version-min=12.0 \
        -fobjc-arc \
        -fapplication-extension \
        -framework AppKit \
        -framework Foundation \
        -Wl,-e,_NSExtensionMain \
        "$SHARE_SOURCE_DIR/ShareViewController.m" \
        -o "$SHARE_BUILD_DIR/HiRodropShare-$ARCH"
done
xcrun lipo -create \
    "$SHARE_BUILD_DIR/HiRodropShare-arm64" \
    "$SHARE_BUILD_DIR/HiRodropShare-x86_64" \
    -output "$SHARE_MACOS_DIR/HiRodropShare"
cp "$SHARE_SOURCE_DIR/Info.plist" "$SHARE_CONTENTS_DIR/Info.plist"
cp -R "$SHARE_SOURCE_DIR/localizations/." "$SHARE_RESOURCES_DIR/"

trap 'rm -rf "$SHARE_BUILD_DIR"' EXIT
cp "$APP_ICON" "$RESOURCES_DIR/HiRodrop.icns"
cp "$RESOURCES_DIR/HiRodrop.icns" "$SHARE_RESOURCES_DIR/HiRodrop.icns"
cp "$PROJECT_DIR/LICENSE" "$RESOURCES_DIR/LICENSE.txt"
cp "$PROJECT_DIR/THIRD_PARTY_LICENSES.md" "$RESOURCES_DIR/THIRD_PARTY_LICENSES.md"
cp "$PROJECT_DIR/docs/protocol-sources.md" "$RESOURCES_DIR/PROTOCOL_SOURCES.md"
zsh "$SCRIPT_DIR/generate-third-party-notices.sh" "$RESOURCES_DIR/THIRD_PARTY_NOTICES.txt"

SIGN_ARGS=(--force --options runtime --sign "$SIGN_IDENTITY")
if [[ "$SIGN_IDENTITY" != "-" ]]; then
    SIGN_ARGS+=(--timestamp)
fi
codesign "${SIGN_ARGS[@]}" \
    --entitlements "$SHARE_SOURCE_DIR/ShareExtension.entitlements" \
    "$SHARE_APP_DIR"
codesign "${SIGN_ARGS[@]}" "$APP_DIR"

print "Built $APP_DIR"
if [[ "$SIGN_IDENTITY" == "-" ]]; then
    print "This is ad-hoc signed with Hardened Runtime for local testing."
    print "Distribution requires an Apple Developer ID signature and notarization."
else
    print "Signed with: $SIGN_IDENTITY"
fi
