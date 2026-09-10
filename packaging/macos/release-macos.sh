#!/bin/zsh
set -euo pipefail

SCRIPT_DIR="${0:A:h}"
PROJECT_DIR="${SCRIPT_DIR:h:h}"
DIST_DIR="$PROJECT_DIR/dist"
APP_DIR="$DIST_DIR/HiRodrop.app"
DMG_PATH="$DIST_DIR/HiRodrop-macOS-universal.dmg"
SIGN_IDENTITY="${HIRODROP_SIGN_IDENTITY:--}"
NOTARY_PROFILE="${HIRODROP_NOTARY_PROFILE:-}"

cd "$PROJECT_DIR"
HIRODROP_SIGN_IDENTITY="$SIGN_IDENTITY" zsh "$SCRIPT_DIR/build-app.sh"

STAGING_DIR="$(mktemp -d)"
trap 'rm -rf "$STAGING_DIR"' EXIT
cp -R "$APP_DIR" "$STAGING_DIR/HiRodrop.app"
ln -s /Applications "$STAGING_DIR/Applications"

rm -f "$DMG_PATH"
hdiutil create \
    -volname HiRodrop \
    -srcfolder "$STAGING_DIR" \
    -format UDZO \
    -ov \
    "$DMG_PATH"

if [[ "$SIGN_IDENTITY" != "-" ]]; then
    codesign --force --timestamp --sign "$SIGN_IDENTITY" "$DMG_PATH"
fi

if [[ -n "$NOTARY_PROFILE" ]]; then
    if [[ "$SIGN_IDENTITY" == "-" ]]; then
        print -u2 "HIRODROP_NOTARY_PROFILE requires a Developer ID signature."
        exit 1
    fi
    xcrun notarytool submit "$DMG_PATH" \
        --keychain-profile "$NOTARY_PROFILE" \
        --wait
    xcrun stapler staple "$DMG_PATH"
    spctl --assess --type execute --verbose=2 "$APP_DIR"
    spctl --assess --type install --verbose=2 "$DMG_PATH"
    print "Built, notarized, and stapled $DMG_PATH"
else
    print "Built $DMG_PATH"
    if [[ "$SIGN_IDENTITY" == "-" ]]; then
        print "This local-test DMG is ad-hoc signed and is not notarized."
    else
        print "Set HIRODROP_NOTARY_PROFILE to notarize and staple this release."
    fi
fi

# Release output contains only the distributable DMG. Developers can call
# build-app.sh directly when they need an unpacked app for local testing.
rm -rf "$APP_DIR"
rm -f "$DIST_DIR/HiRodrop-macOS-universal.zip" "$DIST_DIR/HiRodrop-macOS.dmg"
