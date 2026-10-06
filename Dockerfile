# Optional convenience image. The single binary remains the primary install method.
# Build the binary first with `make release` (on Linux x86_64 — the binary is copied as-is).
FROM debian:bookworm-slim

# Fixed non-root UID/GID so the host ./data bind mount can be chowned to match.
ARG APP_UID=1000
ARG APP_GID=1000

RUN groupadd --system --gid ${APP_GID} zeroboard \
    && useradd --system --uid ${APP_UID} --gid zeroboard --no-create-home zeroboard

WORKDIR /app

COPY backend/target/release/zeroboard /app/zeroboard

RUN mkdir -p /app/data/attachments \
    && chown -R zeroboard:zeroboard /app/data

USER zeroboard

EXPOSE 3000

CMD ["/app/zeroboard"]
