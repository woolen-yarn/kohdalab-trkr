# SignPath application preparation

Status: preparation only. This repository has no SignPath organization ID,
project, policy, GitHub App installation, API token, certificate, or accepted
Foundation sponsorship configured. The CI workflow never submits a signing
request and never exposes a signing secret.

The public application and review repository is
[woolen-yarn/kohdalab-trkr](https://github.com/woolen-yarn/kohdalab-trkr).
`release.yml` rebuilds native helpers and Rust binaries from this source on
GitHub-hosted Windows and macOS runners. It uploads unsigned release artifacts
and a separate Windows `signpath-input-*` workflow artifact. That artifact has
only the three first-party PE files in `application-pe.xml`, their SHA-256
record, and GitHub-provided artifact origin metadata.

The GPL GPIB helper is intentionally excluded. The NSIS Setup executable is
uploaded separately as a review sample because the installer/uninstaller
boundary must be approved before an installer policy is enabled.

## Steps after SignPath acceptance

1. Confirm the public repository, maintainers, signing approvers, and MFA in
   SignPath. Install the SignPath GitHub App and link the predefined GitHub.com
   trusted build system to the project.
2. Create a release signing policy that requires manual approval and
   GitHub-hosted runners. Review [`application-pe.xml`](application-pe.xml) and
   the separate installer boundary before enabling either policy.
3. Add the SignPath organization ID, project slug, signing-policy slug, and
   restricted token to a protected GitHub environment. Use the official
   `signpath/github-action-submit-signing-request@v3` only on the artifact ID
   produced by the same trusted workflow; do not upload a locally built archive.
4. Sign the exact allowlisted files, verify the signer and timestamp, then build
   the final portable archive and Setup executable from those signed bytes.
   The generated NSIS uninstaller needs an approved signing hook before Setup
   can be represented as a fully signed distribution.
5. Update the release notes with the actual signature status and publisher.
   Do not claim SignPath sponsorship or publish its credit before acceptance.

SignPath requires a GitHub Actions artifact before a trusted signing submission
and evaluates GitHub origin information for the workflow. See the official
[GitHub trusted-build integration](https://docs.signpath.io/trusted-build-systems/github)
and [artifact configuration reference](https://docs.signpath.io/artifact-configuration/reference).
