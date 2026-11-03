# Code signing (optional, credential-gated)

Windows installers are built in CI and are unsigned until a certificate is added as **repository
secrets** (GitHub → repo → Settings → Secrets and variables → Actions → New repository secret). Once
present, the next CI run signs automatically and the *Verify signatures* step fails the build if a
signature is not valid. The macOS build is made and signed locally (below).

## macOS (Developer ID + notarization) — local

The macOS build is made on a Mac, not in CI, so it is signed there. Requires an Apple Developer
Program membership and the *Developer ID Application* certificate in your login keychain. Tauri signs
and notarizes during the build when these are set in the shell:

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"  # security find-identity -v -p codesigning
export APPLE_ID="you@example.com"           # Apple ID used for notarization
export APPLE_PASSWORD="xxxx-xxxx-xxxx-xxxx" # app-specific password (appleid.apple.com → Sign-In and Security)
export APPLE_TEAM_ID="TEAMID"
npx tauri build
codesign --verify --deep --strict --verbose=2 "target/release/bundle/macos/EMCOMM LOG.app"
spctl --assess --type execute --verbose "target/release/bundle/macos/EMCOMM LOG.app"
```

## Windows (Authenticode)

Requires a code-signing certificate from a CA (OV or EV). EV certificates on hardware tokens or cloud
HSMs cannot be exported as `.pfx`; for those, use the vendor's signing tool via Tauri's
`bundle.windows.signCommand` instead.

| Secret | Value |
|---|---|
| `WINDOWS_CERTIFICATE` | Base64 of the `.pfx` (`[Convert]::ToBase64String([IO.File]::ReadAllBytes("cert.pfx"))`) |
| `WINDOWS_CERTIFICATE_PASSWORD` | The `.pfx` password |

CI imports the certificate, passes its thumbprint to the Tauri bundler (SHA-256, DigiCert timestamp),
and checks `Get-AuthenticodeSignature` is `Valid` for the `.msi` and `-setup.exe`.

## Linux

`.deb`, `.rpm` and `.AppImage` are distributed unsigned. Publish the SHA-256 checksums from CI next to
them (`SHA256SUMS.txt` in each artifact).
