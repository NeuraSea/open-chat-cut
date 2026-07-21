# Security Policy

Please report suspected vulnerabilities privately through the repository
owner's security-reporting channel. Do not include daemon tokens, provider
credentials, private media, transcripts, or reproduction projects in a public
issue.

Security-sensitive areas include loopback authentication, CSRF and Origin
checks, authorized import/export roots, symlink traversal, remote-download
SSRF, provider redirects, subtitle parsing, prompt injection, MG/JSX sandboxing,
and secret redaction. Include the affected commit, platform, minimal steps, and
impact when reporting.

Only the current `main` branch is supported before the first stable release.
