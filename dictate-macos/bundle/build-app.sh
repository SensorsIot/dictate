#!/usr/bin/env bash
# Builds dictate.app as a universal binary and zips it as target/dictate-macos.zip.
#
# Signed with the Developer ID and notarized when the signing secrets are in the
# environment (GitHub Actions secrets; never in the repository). Without them
# the bundle gets an ad-hoc signature, which runs, but which macOS treats as a
# new application on every build, so its permissions have to be granted again.
#
# Usage: build-app.sh <version>
set -euo pipefail

version="$1"
cd "$(dirname "$0")/../.."

for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release -p dictate-macos --target "$target" ${LOCKED:+--locked}
done

app=target/dictate.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
lipo -create -output "$app/Contents/MacOS/dictate" \
    target/aarch64-apple-darwin/release/dictate-macos \
    target/x86_64-apple-darwin/release/dictate-macos
sed "s/@VERSION@/$version/g" dictate-macos/bundle/Info.plist > "$app/Contents/Info.plist"

entitlements=dictate-macos/bundle/dictate.entitlements

if [ -n "${MACOS_CERTIFICATE:-}" ]; then
    # A throwaway keychain holds the certificate for this job only.
    keychain="$RUNNER_TEMP/signing.keychain-db"
    keychain_password="$(uuidgen)"
    security create-keychain -p "$keychain_password" "$keychain"
    security set-keychain-settings -lut 21600 "$keychain"
    security unlock-keychain -p "$keychain_password" "$keychain"
    echo "$MACOS_CERTIFICATE" | base64 --decode > "$RUNNER_TEMP/certificate.p12"
    security import "$RUNNER_TEMP/certificate.p12" -k "$keychain" -P "$MACOS_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
    rm -f "$RUNNER_TEMP/certificate.p12"
    security set-key-partition-list -S apple-tool:,apple: -k "$keychain_password" "$keychain" > /dev/null
    security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')

    codesign --force --options runtime --timestamp --entitlements "$entitlements" \
        --sign "Developer ID Application" "$app"

    ditto -c -k --keepParent "$app" target/notarize.zip
    echo "$APPLE_API_KEY" | base64 --decode > "$RUNNER_TEMP/AuthKey.p8"
    xcrun notarytool submit target/notarize.zip \
        --key "$RUNNER_TEMP/AuthKey.p8" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER_ID" \
        --wait | tee target/notarize.log
    rm -f "$RUNNER_TEMP/AuthKey.p8" target/notarize.zip
    grep -q "status: Accepted" target/notarize.log

    xcrun stapler staple "$app"
    security delete-keychain "$keychain"
else
    echo "::warning::No signing certificate in this run: dictate.app has an ad-hoc signature only."
    codesign --force --options runtime --entitlements "$entitlements" --sign - "$app"
fi

codesign --verify --strict --verbose=2 "$app"
ditto -c -k --keepParent "$app" target/dictate-macos.zip
ls -l target/dictate-macos.zip
