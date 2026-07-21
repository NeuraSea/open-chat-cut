# Distribution, releases, and support

OpenChatCut has three deliberately separate delivery paths. They share the
same Rust operation engine, SQLite project format, managed media store, Web
scene graph, worker protocol, and Codex MCP bundle.

| Path | Intended user | Project/media location | Status |
| --- | --- | --- | --- |
| Managed single-user deployment | A team operating one isolated editor node per user/workspace | Dedicated server volume | Supported by `docker-compose.hosted.yml`; not multi-tenant SaaS |
| Clone and build | Contributors and advanced local users | `~/.openchatcut` on the workstation | Supported by one command on macOS/Linux/Windows |
| Portable Release client | Users who do not want Rust, Bun, or Docker toolchains | `~/.openchatcut` on the workstation | Versioned native daemon + standalone Web bundle; first install prepares Python worker/Chromium |

The current “client” is a small native daemon plus a bundled standalone Web
editor opened in the system browser. The GPUI app under `apps/desktop` remains
a development shell and is **not** presented as a finished editor. A future
signed native window can replace the browser without changing project state or
the operation engine.

## 1. Clone and build

Prerequisites are Git, Docker Desktop/Compose, Rust, Python 3.11+, Node.js 20+,
and optionally Codex CLI. The source installer builds the daemon, creates the
worker environment, installs Chromium, builds the Web image, installs the
Codex plugin when available, and starts the product:

```bash
git clone https://github.com/OWNER/open-chat-cut.git
cd open-chat-cut
./scripts/install.sh
```

```powershell
git clone https://github.com/OWNER/open-chat-cut.git
cd open-chat-cut
.\scripts\install.ps1
```

Use `--without-ml` / `-WithoutMl` for a smaller install that keeps render and
export support but omits local transcription/diarization/denoise packages.
After `codex login`, rerun the plugin installer and open a new Codex task.

## 2. Portable GitHub Release client

Every `v*` tag builds these checksum-manifested archives:

- `openchatcut-VERSION-aarch64-apple-darwin.tar.gz`
- `openchatcut-VERSION-x86_64-unknown-linux-gnu.tar.gz`
- `openchatcut-VERSION-x86_64-pc-windows-msvc.zip`

The archive includes the native daemon, a bundled JavaScript runtime (official
release jobs use Bun), Next.js standalone Web
editor, MG compiler and parser, media-worker source, Codex plugin marketplace,
licenses, and a per-file SHA-256 manifest. Rust, Bun, Node, and Docker are not
required to run this package. Python 3.11+ is required on first install; FFmpeg
is installed when missing. An existing local Chrome/Chromium is reused for
headless rendering, otherwise the installer downloads Playwright's pinned
Chromium runtime. The first install therefore needs network access and can be
large when ML extras are enabled.

Verify both layers before installation:

```bash
shasum -a 256 -c openchatcut-VERSION-TARGET.tar.gz.sha256
tar -xzf openchatcut-VERSION-TARGET.tar.gz
python3 openchatcut-VERSION-TARGET/scripts/release/verify-release-bundle.py \
  openchatcut-VERSION-TARGET
openchatcut-VERSION-TARGET/install.sh
```

On Windows, verify the `.sha256`, extract the zip, then run:

```powershell
python .\openchatcut-VERSION-TARGET\scripts\release\verify-release-bundle.py `
  .\openchatcut-VERSION-TARGET
powershell -ExecutionPolicy Bypass -File .\openchatcut-VERSION-TARGET\install.ps1
```

Unix installs create `~/.local/bin/openchatcut`; Windows installs
`%LOCALAPPDATA%\OpenChatCut\openchatcut.ps1`. Useful commands are `open`,
`start`, `stop`, `restart`, `status`, and `logs`. Set
`OPENCHATCUT_WEB_PORT` before `start`/`open` when port 3100 is occupied. The
daemon stays on `127.0.0.1:3210` because the portable browser bundle is compiled
for that loopback endpoint; changing only the Web port is supported.

Release archives are currently portable preview artifacts. macOS notarization,
Windows Authenticode/MSIX, and Linux package-repository signing require project
owner certificates and are release-blocking gates before calling the native
download “stable”. Checksums and the internal manifest detect corruption but
are not substitutes for publisher signatures.

## 3. Managed single-user hosted deployment

The daemon is not a multi-tenant service. A managed offering must allocate a
dedicated Compose project and persistent volumes per user/workspace, put Caddy
or an equivalent identity-aware proxy in front, and never publish daemon port
3210. The checked-in hosted profile enables public browser origins only when
all of these conditions are explicit:

- container mode is enabled;
- `OPENCHATCUT_HOSTED_ORIGIN` is an HTTPS origin without a path;
- Caddy basic authentication (or a stronger upstream access product) gates
  every route;
- Caddy rewrites only the upstream `Host` header while preserving the browser
  `Origin` checked by the daemon;
- browser cookies are emitted with `Secure`, `HttpOnly`, `SameSite=Strict`.

```bash
cp deploy/hosted/.env.example .env.hosted
# Set the domain, user, and a real Caddy password hash. Escape each `$` in a
# Compose .env value as `$$`.
docker compose --env-file .env.hosted -f docker-compose.hosted.yml up -d --build
```

The hosted Web image is compiled with `NEXT_PUBLIC_OPENCHATCUT_API_URL=same-origin`;
Caddy routes `/api/v1/*` to the dedicated daemon and all other requests to the
Web editor. Codex OAuth is intentionally unavailable in the full Docker image.
Use a configured private OpenAI-compatible planner or operate Codex through a
local Release client. Public shared accounts, billing, quotas, cross-user asset
access, and real-time collaboration remain out of scope.

## Release automation and gates

`.github/workflows/release.yml` runs on `v*` tags. It builds the three portable
clients, installs each artifact into an isolated home, starts its bundled
daemon/Web/Worker, creates a project, stops and restarts, and verifies the same
revision and document hash. It then publishes multi-architecture
`web`, `web-hosted`, and `daemon-full` OCI images to GHCR, verifies archive
checksums, and attaches the artifacts to a GitHub Release. A manual dispatch
builds dry-run artifacts but does not publish a GitHub Release or containers.

A stable release additionally requires:

1. the normal three-platform CI and browser/package roundtrip are green;
2. the 30-second export and ProRes-alpha fixtures pass;
3. portable archives install on clean machines and survive restart/upgrade;
4. hosted Compose is tested behind real TLS and outer authentication;
5. macOS, Windows, and Linux artifacts are signed with project-owner keys;
6. provider smoke tests are explicitly enabled only with owner-controlled keys;
7. release notes list model downloads, disk requirements, known limitations,
   database backup instructions, and any schema migration.

## Upgrade, backup, uninstall, and support

Projects and credentials are not stored inside an application version. Back up
`~/.openchatcut` (or the hosted `openchatcut-hosted-data` and runtime volumes)
before upgrades. Portable installers place versions side by side and update the
launcher only after the new bundle manifest verifies. Do not delete the prior
version until the daemon starts and project package roundtrip succeeds.

For support, collect only redacted diagnostics:

```bash
./scripts/doctor.sh
openchatcut status
openchatcut logs
```

Never attach `daemon.token`, `providers.json`, Keychain output, project media,
or raw transcript text to an issue. Include OS/architecture, release version,
FFmpeg/Chromium capability status, failing job ID/state/error code, and the
smallest reproducible project package that the user has explicitly reviewed.

To uninstall the portable app, stop it, remove the version directory and
launcher, then optionally remove `~/.openchatcut`. Removing the data directory
permanently deletes projects, managed media, job history, and local settings;
export portable project packages first.
