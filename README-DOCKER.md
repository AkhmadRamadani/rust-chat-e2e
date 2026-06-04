# Self-hosting with Docker

## Quick start

### Development / Preview (with mock OIDC)

```sh
cp .env.example .env   # fill in ADMIN_TOKEN, passwords
docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d
```

Open **http://localhost:3000** — the mock OIDC server is running. Register a tenant with issuer `http://oidc/default`, then issue tokens from the Users tab.

### Production (real OIDC)

```sh
cp .env.example .env   # fill in secrets
docker compose up -d   # NO dev overlay
```

Then register your real OIDC tenant:

```sh
curl -X POST http://localhost:3000/api/admin/tenants \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name":"My App","oidc_issuer":"https://your-idp.example.com"}'
```

Alternatively, you can visit **http://localhost:3000/admin** to configure tenants using the Admin Dashboard UI.

Supported providers: Auth0, Keycloak, Okta, Cognito, Firebase, Supabase, or any standard OIDC provider. The API fetches JWKS from `{oidc_issuer}/.well-known/jwks.json`.

---

## File reference

| File | Purpose |
|---|---|
| `docker-compose.yml` | **Production** — postgres, redis, api, web (Next.js), client (nginx) |
| `docker-compose.dev.yml` | **Dev overlay** — adds mock OIDC (mock-oauth2-server + nginx proxy) |
| `web/Dockerfile` | Multi-stage builder for the Next.js web application |
| `client/nginx.conf` | Production nginx — proxies `/api` to rust backend, rest to Next.js |
| `client/nginx.dev.conf` | Dev nginx — adds `/oidc` proxy route for mock token issuance |
| `.env.example` | Template for environment variables |

---

## Rebuilding after code changes

```sh
docker compose build api
docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d api   # dev
# or
docker compose up -d api   # prod
```

This guide covers running the full `rust-e2e-chat-api` stack locally or on a server using Docker Compose.

---

## Prerequisites

- [Docker Engine](https://docs.docker.com/engine/install/) ≥ 24
- [Docker Compose](https://docs.docker.com/compose/install/) v2 (`docker compose`, not `docker-compose`)

---

## Quick start

### 1. Configure environment

```sh
cp .env.example .env
```

Edit `.env` and set at minimum:

| Variable          | Description                                    |
|-------------------|------------------------------------------------|
| `POSTGRES_PASSWORD` | PostgreSQL password                          |
| `REDIS_PASSWORD`  | Redis password                                 |
| `ADMIN_TOKEN`     | Secret for `/admin/*` routes                   |

Generate a strong admin token:

```sh
openssl rand -hex 32
```

### 2. Provide TLS certificates

The API server uses QUIC/HTTP-3, which **requires a TLS certificate**.

Create the `certs/` directory and place your certificate files there:

```sh
mkdir -p certs
```

**Option A — Self-signed (local dev only):**

```sh
openssl req -x509 -newkey rsa:4096 -keyout certs/key.pem \
  -out certs/cert.pem -days 365 -nodes \
  -subj "/CN=localhost"
```

**Option B — Let's Encrypt (production):**

Use [Certbot](https://certbot.eff.org/) or [Caddy](https://caddyserver.com/) to obtain a certificate, then copy:

```sh
cp /etc/letsencrypt/live/yourdomain.com/fullchain.pem certs/cert.pem
cp /etc/letsencrypt/live/yourdomain.com/privkey.pem   certs/key.pem
```

### 3. Start the stack

```sh
docker compose up -d
```

This will:
1. Start PostgreSQL and Redis
2. Run database migrations (`sqlx migrate run`)
3. Build and start the API server

### 4. Verify

Check all containers are healthy:

```sh
docker compose ps
```

View API logs:

```sh
docker compose logs -f api
```

Check the health endpoint (if your server entry point wires up the HTTP listener):

```sh
curl http://localhost:8080/health
# Or via proxy: curl http://localhost:3000/api/health
```

Check Prometheus metrics:

```sh
curl http://localhost:8080/metrics
# Or via proxy: curl http://localhost:3000/api/metrics
```

---

## Rebuilding after code changes

```sh
docker compose build api
docker compose up -d api
```

---

## Stopping and cleaning up

Stop containers (keep data volumes):

```sh
docker compose down
```

Stop and remove all data:

```sh
docker compose down -v
```

---

## Architecture

```
┌─────────────────────────────────────────────────┐
│  Host / Reverse Proxy                           │
│                                                 │
│  TCP :3000  ──────►  nginx                      │
│                        ├── /api/* ─► api:8080   │
│                        └── /*     ─► web:3000   │
└─────────────────────────────────────────────────┘
                          │
              ┌───────────┴──────────┐
              ▼                      ▼
         postgres:5432           redis:6379
         (PgPool)               (token store)
```

---

## Environment variables reference

| Variable          | Default         | Description                              |
|-------------------|-----------------|------------------------------------------|
| `DATABASE_URL`    | —               | Full PostgreSQL connection string        |
| `REDIS_URL`       | —               | Full Redis connection string             |
| `ADMIN_TOKEN`     | **required**    | Bearer token for `/admin/*` routes       |
| `TLS_CERT_PATH`   | `/app/certs/cert.pem` | Path to TLS certificate PEM        |
| `TLS_KEY_PATH`    | `/app/certs/key.pem`  | Path to TLS private key PEM        |
| `BIND_ADDR`       | `0.0.0.0:4433`  | QUIC listener bind address               |
| `RUST_LOG`        | `info`          | Tracing filter (e.g. `api=debug,info`)   |

---

## Production checklist

- [ ] Replace self-signed cert with a CA-signed certificate
- [ ] Set strong random values for `POSTGRES_PASSWORD`, `REDIS_PASSWORD`, `ADMIN_TOKEN`
- [ ] Restrict `RUST_LOG` to `warn` or `error` in production for lower noise
- [ ] Place a reverse proxy (Caddy, nginx, Traefik) in front for HTTPS termination and rate limiting
- [ ] Enable PostgreSQL and Redis authentication (both enabled by default in this Compose file)
- [ ] Back up `postgres_data` and `redis_data` volumes regularly
