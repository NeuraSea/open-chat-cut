ARG WEB_ARTIFACT_STAGE=builder

FROM rust:1.88-slim-bookworm AS wasm-builder

WORKDIR /app

ARG WASM_BINDGEN_VERSION=0.2.116
# The slim Rust image intentionally omits download tooling. Install the small
# verified bootstrap surface explicitly so a clean Docker cache behaves the
# same as CI and does not rely on an older inherited layer that happened to
# contain curl.
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/*
RUN set -eux; \
  case "$(uname -m)" in \
    x86_64) \
      wasm_arch=x86_64; \
      wasm_sha=f10d233eb87aaaed4a6a3af3d52ab9a3420ab35103c18fe2c6a69527bfbf7035 \
      ;; \
    aarch64|arm64) \
      wasm_arch=aarch64; \
      wasm_sha=8366dba5c663f7e56c9e14752e69424686af9244389376bb7bbcca12604d0505 \
      ;; \
    *) echo "Unsupported wasm-bindgen build architecture: $(uname -m)" >&2; exit 1 ;; \
  esac; \
  archive="wasm-bindgen-${WASM_BINDGEN_VERSION}-${wasm_arch}-unknown-linux-musl.tar.gz"; \
  curl -fsSL \
    "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/${WASM_BINDGEN_VERSION}/${archive}" \
    -o "/tmp/${archive}"; \
  echo "${wasm_sha}  /tmp/${archive}" | sha256sum -c -; \
  tar -xzf "/tmp/${archive}" -C /tmp; \
  install -m 0755 "/tmp/${archive%.tar.gz}/wasm-bindgen" /usr/local/bin/wasm-bindgen; \
  rustup target add wasm32-unknown-unknown

COPY Cargo.toml Cargo.lock ./
# Cargo resolves every workspace manifest before selecting `opencut-wasm`.
# Keep the Docker workspace structurally identical to the checkout; otherwise
# a clean cache fails as soon as a new native member is added at the root.
COPY apps/daemon/ apps/daemon/
COPY apps/desktop/ apps/desktop/
COPY rust/ rust/

# The Docker Web editor must consume this repository's Rust renderer rather
# than the published Classic baseline. This keeps alpha/export behavior and
# every other platform-neutral renderer fix in the Rust source of truth.
RUN cargo build \
  --release \
  --locked \
  --target wasm32-unknown-unknown \
  -p opencut-wasm \
  && mkdir -p rust/wasm/pkg \
  && wasm-bindgen \
    --target bundler \
    --typescript \
    --out-dir rust/wasm/pkg \
    target/wasm32-unknown-unknown/release/opencut_wasm.wasm \
  && cp rust/wasm/package.web.json rust/wasm/pkg/package.json

FROM oven/bun:1.3.14-alpine AS base

FROM base AS js-deps

WORKDIR /app

ARG NEXT_PUBLIC_OPENCHATCUT_API_URL=http://127.0.0.1:3210/api/v1

COPY package.json package.json
COPY bun.lock bun.lock
COPY turbo.json turbo.json

COPY apps/web/package.json apps/web/package.json
COPY packages/provider-kit/package.json packages/provider-kit/package.json
COPY packages/mg-runtime/package.json packages/mg-runtime/package.json
RUN bun install --frozen-lockfile

FROM js-deps AS builder

COPY --from=wasm-builder /app/rust/wasm/pkg /app/rust/wasm/pkg
RUN wasm_package=$(readlink -f apps/web/node_modules/opencut-wasm) \
  && test -n "$wasm_package" \
  && rm -rf "$wasm_package" \
  && cp -R /app/rust/wasm/pkg "$wasm_package"

COPY apps/web/ apps/web/

ENV NODE_ENV=production
ENV NEXT_TELEMETRY_DISABLED=1

ENV NEXT_PUBLIC_SITE_URL="http://127.0.0.1:3100"
ENV NEXT_PUBLIC_OPENCHATCUT_API_URL=$NEXT_PUBLIC_OPENCHATCUT_API_URL

WORKDIR /app/apps/web
RUN bun run build

# Developer/CI escape hatch for a package already produced by
# `wasm-bindgen`. The default image never uses this stage; it exists so a
# local Rust build can be verified without downloading a second Rust toolchain
# image. Select it with WEB_ARTIFACT_STAGE=builder-prebuilt.
FROM js-deps AS builder-prebuilt

COPY rust/wasm/pkg /app/rust/wasm/pkg
RUN wasm_package=$(readlink -f apps/web/node_modules/opencut-wasm) \
  && test -n "$wasm_package" \
  && rm -rf "$wasm_package" \
  && cp -R /app/rust/wasm/pkg "$wasm_package"

COPY apps/web/ apps/web/

ENV NODE_ENV=production
ENV NEXT_TELEMETRY_DISABLED=1
ENV NEXT_PUBLIC_SITE_URL="http://127.0.0.1:3100"
ENV NEXT_PUBLIC_OPENCHATCUT_API_URL=$NEXT_PUBLIC_OPENCHATCUT_API_URL

WORKDIR /app/apps/web
RUN bun run build

FROM ${WEB_ARTIFACT_STAGE} AS web-artifacts

# Production image
FROM base AS runner
WORKDIR /app

ENV NODE_ENV=production
ENV NEXT_TELEMETRY_DISABLED=1

RUN addgroup --system --gid 1001 nodejs
RUN adduser --system --uid 1001 nextjs

COPY --from=web-artifacts --chown=nextjs:nodejs /app/apps/web/public ./apps/web/public
COPY --from=web-artifacts --chown=nextjs:nodejs /app/apps/web/.next/standalone ./
COPY --from=web-artifacts --chown=nextjs:nodejs /app/apps/web/.next/static ./apps/web/.next/static

RUN chown nextjs:nodejs apps

USER nextjs

EXPOSE 3000

ENV PORT=3000
ENV HOSTNAME="0.0.0.0"

CMD ["bun", "apps/web/server.js"]
