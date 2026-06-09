# macOS Signing and Notarization

## Current state (v0.1)

The `release.yml` CI produces an **unsigned** `.dmg` for macOS.
Users who download it will see a Gatekeeper warning on first launch:

> "colosseum" can't be opened because it is from an unidentified developer.

They can bypass this once via **System Settings → Privacy & Security → Open Anyway**,
or from the terminal:

```bash
xattr -d com.apple.quarantine /Applications/Colosseum.app
```

## Signing (future)

To distribute without the Gatekeeper warning you need an Apple Developer account ($99/yr)
and a **Developer ID Application** certificate.

### 1 — Sign the `.app` bundle

```bash
# List available identities
security find-identity -v -p codesigning

# Sign (replace with your actual identity)
codesign \
  --deep \
  --force \
  --options runtime \
  --sign "Developer ID Application: Your Name (TEAMID)" \
  Colosseum.app
```

### 2 — Package into a signed DMG

```bash
# Create a writable DMG, then convert to compressed read-only
hdiutil create -volname "Colosseum" -srcfolder Colosseum.app \
  -ov -format UDRW staging.dmg
hdiutil convert staging.dmg -format UDZO -o colosseum.dmg

# Sign the DMG itself
codesign --sign "Developer ID Application: Your Name (TEAMID)" colosseum.dmg
```

### 3 — Notarize

```bash
xcrun notarytool submit colosseum.dmg \
  --apple-id "you@example.com" \
  --team-id "TEAMID" \
  --password "@keychain:notarytool-password" \
  --wait

xcrun stapler staple colosseum.dmg
```

### 4 — Add signing to release.yml

Store your `APPLE_CERTIFICATE` (base-64 encoded .p12), `APPLE_CERTIFICATE_PASSWORD`,
`APPLE_ID`, `APPLE_TEAM_ID`, and `APPLE_ID_PASSWORD` as GitHub Actions secrets,
then add a signing step before the `hdiutil create` call:

```yaml
- name: Import certificate
  env:
    APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
    APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
  run: |
    echo "$APPLE_CERTIFICATE" | base64 --decode > cert.p12
    security create-keychain -p "" build.keychain
    security import cert.p12 -k build.keychain -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
    security set-keychain-settings -lut 21600 build.keychain
    security default-keychain -s build.keychain
    security unlock-keychain -p "" build.keychain
    security set-key-partition-list -S apple-tool:,apple: -s -k "" build.keychain

- name: Sign app bundle
  run: |
    codesign --deep --force --options runtime \
      --sign "${{ secrets.APPLE_TEAM_ID }}" \
      dmg-staging/Colosseum.app
```
