# Maintainers

- **Sean Shaffer** (GitHub: SeanShaffer88) — creator and sole
  maintainer: design, releases, issue triage, and final say on
  scope.

## Release process (summary)

1. Work lands on `main` through the gated CI (fmt + clippy
   `-D warnings` + full tests).
2. A version is tagged `v{VERSION}` only with the maintainer's
   explicit confirmation; fixes are batched into grouped releases
   rather than shipped one at a time.
3. The release workflow builds the gated tree, packages the
   installer and the portable zip under the V4.0 asset names,
   writes `SHA256SUMS.txt`, and attaches them (plus the `drinks.json`
   update pack) to the GitHub release.
4. Published assets are re-downloaded and hash-checked against
   `SHA256SUMS.txt` before the release is called verified.

See [GOVERNANCE.md](GOVERNANCE.md) for how decisions are made.
