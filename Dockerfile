# syntax=docker/dockerfile:1.7
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY api api
COPY spider spider
COPY shared_crawler_api shared_crawler_api
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release --workspace \
    && install -m 755 -D target/release/api /out/api \
    && install -m 755 -D target/release/spider /out/spider

FROM debian:bookworm-slim AS api
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 app
COPY --from=build /out/api /usr/local/bin/api
USER app
ENTRYPOINT ["api"]

FROM debian:bookworm-slim AS spider
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates chromium curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 app
COPY --from=build /out/spider /usr/local/bin/spider
USER app
ENV CHROME_EXECUTABLE=/usr/bin/chromium
ENTRYPOINT ["spider"]
