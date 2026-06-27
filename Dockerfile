FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY api api
COPY spider spider
COPY shared_crawler_api shared_crawler_api
RUN cargo build --release --workspace

FROM debian:bookworm-slim AS api
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 app
COPY --from=build /src/target/release/api /usr/local/bin/api
USER app
ENTRYPOINT ["api"]

FROM debian:bookworm-slim AS spider
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates chromium curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 app
COPY --from=build /src/target/release/spider /usr/local/bin/spider
USER app
ENV CHROME_EXECUTABLE=/usr/bin/chromium
ENTRYPOINT ["spider"]
