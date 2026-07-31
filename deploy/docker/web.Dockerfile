FROM node:22.18.0-alpine3.22 AS builder

WORKDIR /app
COPY apps/web/package.json apps/web/package-lock.json ./
RUN npm ci
COPY apps/web ./

ARG VITE_BASE_PATH=/
ENV VITE_BASE_PATH=${VITE_BASE_PATH}
RUN npm run build

FROM caddy:2.10.2-alpine

ARG STRIFE_REVISION=unknown
LABEL org.opencontainers.image.title="Strife Web" \
      org.opencontainers.image.revision="${STRIFE_REVISION}" \
      org.opencontainers.image.source="https://github.com/s4njee/strife"

RUN setcap -r /usr/bin/caddy

COPY deploy/docker/Caddyfile /etc/caddy/Caddyfile
COPY --from=builder /app/dist /srv
