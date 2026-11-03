# Code signing (optional, credential-gated)

CI builds unsigned installers until signing credentials are added as **repository secrets**
(GitHub → repo → Settings → Secrets and variables → Actions → New repository secret). Once present,
the next CI run signs automatically and the *Verify signatures* step fails the build if a signature is
not valid. Nothing else needs to change.

## macOS (Developer ID + notarization)

Requires an Apple Developer Program membership.

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | Base64 of your *Developer ID Application* certificate exported as `.p12` (`base64 -i cert.p12 \| pbcopy`) |
| `APPLE_CERTIFICATE_PASSWORD` | Password you set when exporting the `.p12` |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Your Name (TEAMID)` (`security find-identity -v -p codesigning`) |
| `APPLE_ID` | Apple ID email used for notarization |
| `APPLE_PASSWORD` | An app-specific password for that Apple ID (appleid.apple.com → Sign-In and Security) |
| `APPLE_TEAM_ID` | Your 10-character Team ID |

Verification in CI: `codesign --verify --deep --strict` and `spctl --assess` (Gatekeeper).

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
