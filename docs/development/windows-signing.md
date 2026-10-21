# Windows code-signing preparation

Windows release artifacts are currently unsigned. The release workflow rebuilds
them on GitHub-hosted Windows runners, uploads checksummed unsigned artifacts,
and stages a narrow SignPath input archive. It does not contain a SignPath token
or signing submission step.

The only proposed first-party signing inputs are `kohdalab-trkr.exe`,
`kohdalab-trkr-demo.exe`, and `kohdalab-visa-helper.exe`. The workflow fails if
the GPL GPIB helper enters that input. The installer is a separate provider
review sample because NSIS generates an uninstaller during compilation.

After SignPath accepts the project, a protected release workflow should submit
the workflow artifact ID to SignPath, wait for manual approval, download the
signed result, verify the publisher and timestamp with
`Get-AuthenticodeSignature`, and package checksums from the signed bytes. The
installer and generated uninstaller need an approved signing integration before
they can be advertised as signed.

Do not disable SmartScreen, Defender, or Windows security controls to work
around an unsigned-download warning. Obtain releases from GitHub and verify the
published SHA-256 sums.

See [the policy](../../CODE_SIGNING_POLICY.md),
[application configuration](../../.signpath/application-pe.xml), and the official
[SignPath GitHub documentation](https://docs.signpath.io/trusted-build-systems/github).
