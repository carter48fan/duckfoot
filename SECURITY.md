# Security

Duckfoot opens files that come from anywhere: camera RAWs, JPEG/PNG/TIFF/WebP/HEIC
photos, ICC profiles, `.cube` looks, `.xmp` and `.duckfoot` sidecars, and presets. A file
that crashes it, hangs it, makes it read or write somewhere it should not, or runs code is
a security bug.

## Reporting

Please report privately, not in a public issue: use **Report a vulnerability** on the
repository's **Security** tab (GitHub private vulnerability reporting). Include the file
that triggers it if you can, the platform, and the Duckfoot version (About & licences in
the app menu).

You should hear back within a week. Fixes ship in the next release, credited to you unless
you would rather not be.

## Supported versions

Only the latest release is supported: fixes are not backported.

## What is done to keep it safe

- Every parser a file reaches is fuzzed weekly (`fuzz/`, `.github/workflows/fuzz.yml`).
- Decoders run under `catch_unwind`, so a file that makes one panic fails to open rather
  than taking the app down.
- Sidecar and preset values are bounded before they reach the GPU; a look's path is checked
  before anything is opened (no network or device paths).
- Duckfoot makes no network connections at all: there are no HTTP or TLS crates in it.
- Releases carry `SHA256SUMS` and a build-provenance attestation for every file
  (`gh attestation verify <file> -R carter48fan/duckfoot`).
- Dependencies are checked by `cargo deny` (licences, advisories, sources) in CI.
