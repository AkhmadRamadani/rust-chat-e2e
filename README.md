# rust-e2e-chat-api

A multi-tenant real-time chat API built in Rust. Supports 1:1 and group conversations, file attachments, per-tenant OIDC authentication, and WebSocket-based live delivery.

## Features

- **Multi-tenant** — multiple independent apps share one deployment with full data isolation
- **Tenant Admin Portal** — Next.js web UI for admins to manage tenants and view usage metrics
- **Self-Registration Portal** — public UI for clients to apply for tenant access
- **Real-time** — WebSocket delivery with offline queue fallback
- **File attachments** — upload images, videos, documents, or any file type
- **OIDC authentication** — bring your own identity provider (Auth0, Keycloak, Okta, Cognito, etc.)
- **REST API** — standard HTTP/1.1, works from any client or browser
- **Observability** — structured JSON logs, Prometheus metrics, health endpoint

---

## Prerequisites

- [Docker Engine](https://docs.docker.com/engine/install/) ≥ 24
- [Docker Compose](https://docs.docker.com/compose/install/) v2 (`docker compose`, not `docker-compose`)
- Git

---

## Installation

### 1. Clone the repository

```sh
git clone <your-repo-url>
cd rust-chat
```

### 2. Configure environment

```sh
cp .env.example .env
```

Open `.env` and set the required values:

```env
POSTGRES_PASSWORD=change-me-strong-password
REDIS_PASSWORD=change-me-redis-password
ADMIN_TOKEN=change-me-admin-token   # generate: openssl rand -hex 32
```

Everything else has sensible defaults for local use.

### 3. Choose your mode

#### Development / Preview (mock OIDC — no external IdP needed)

```sh
docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d
```

This starts the full stack including a mock OIDC server that issues JWTs for any username you provide — perfect for testing without setting up a real identity provider.

#### Production (real OIDC)

```sh
docker compose up -d
```

No mock OIDC is started. You register your real identity provider in Step 5.

### 4. Verify everything is running

```sh
docker compose ps
```

All services should show `healthy` or `running`:

```
NAME                      STATUS
rust-chat-postgres-1      healthy
rust-chat-redis-1         healthy
rust-chat-migrate-1       exited (0)   ← migrations ran successfully
rust-chat-api-1           running
rust-chat-web-1           running
rust-chat-client-1        running
```

Check the health endpoint:

```sh
curl http://localhost:3000/health
# {"quic_listener":"ok","kds_storage":"ok","message_queue":"ok"}
```

---

## Setup (first-time configuration)

### Step 5a — Dev mode: register the mock tenant

1. Open **http://localhost:3000/admin**
2. Sign in with the `$ADMIN_TOKEN` you set in `.env`
3. Click **Create Tenant**
4. Set Name to `Dev Tenant` and OIDC Issuer to `http://oidc/default`
5. Alternatively, register via the API:
   ```sh
   curl -X POST http://localhost:3000/api/admin/tenants \
     -H "Authorization: Bearer your-admin-token" \
     -H "Content-Type: application/json" \
     -d '{"name":"Dev Tenant","oidc_issuer":"http://oidc/default"}'
   ```

### Step 5b — Production: register your real OIDC tenant

Replace the `oidc_issuer` with your provider's issuer URL:

| Provider | Issuer URL format |
|---|---|
| Auth0 | `https://{your-domain}.auth0.com` |
| Keycloak | `https://keycloak.example.com/realms/{realm}` |
| Okta | `https://{your-domain}.okta.com` |
| AWS Cognito | `https://cognito-idp.{region}.amazonaws.com/{pool-id}` |
| Firebase | `https://securetoken.google.com/{project-id}` |
| Supabase | `https://{project-ref}.supabase.co/auth/v1` |

Use the Admin Portal (**http://localhost:3000/admin**) or the API to register your identity provider:

```sh
curl -X POST http://localhost:3000/api/admin/tenants \
  -H "Authorization: Bearer your-admin-token" \
  -H "Content-Type: application/json" \
  -d '{"name":"My App","oidc_issuer":"https://your-idp.example.com"}'
```

Users can also self-register at **http://localhost:3000/register**, which you can then approve in the Admin Dashboard.

The API validates tokens against `{oidc_issuer}/.well-known/jwks.json`.

---

## Using the Dev Client

Open **http://localhost:3000** in your browser.

### Register users (dev mode)

1. Go to the **👤 Users** tab
2. Under **Get Token (Mock OIDC)**, type a username (e.g. `alice`) and click **Issue Token**
3. The token auto-fills — click **Register & Create User**
4. Repeat for a second user (e.g. `bob`)

### Register users (production)

1. Obtain a JWT from your identity provider
2. Go to the **👤 Users** tab → **Register New User**
3. Paste the JWT and the user ID (`sub` claim), click **Register & Create User**

### Start chatting

1. Click **Use in Chat** on Alice's user card — fills in the Chat tab identity
2. Go to the **💬 Chat** tab
3. Fill in Bob's **Recipient User ID** and **Recipient Device ID** (copy it from his user card)
4. Click **Start Conversation**
5. Type a message and press **Enter** or **Send**

To receive Bob's messages: open a second browser tab at `http://localhost:3000`, use Bob's identity, and open the same conversation.

### Send attachments

In any open conversation, click the **📎** button to attach a file. Supports images (inline preview), videos (player), audio (player), and any other file type (download link). Maximum 100 MB per file.

---

## API Overview

All API endpoints are proxied through nginx at `http://localhost:3000/api/`.

### Web UIs

| Path | Description |
|------|-------------|
| `/` | Legacy chat client |
| `/admin` | Tenant Admin Dashboard (requires `ADMIN_TOKEN`) |
| `/register` | Self-Registration Portal (public) |

### Admin (requires `Authorization: Bearer {ADMIN_TOKEN}`)

| Method | Path | Description |
|--------|------|-------------|
| POST | `/admin/tenants` | Create a tenant |
| DELETE | `/admin/tenants/{id}` | Deactivate a tenant |
| PUT | `/admin/tenants/{id}/oidc` | Update OIDC issuer |
| GET | `/admin/tenants/{id}/usage` | Get usage metrics |

### User routes (requires `Authorization: Bearer {jwt}`)

| Method | Path | Description |
|--------|------|-------------|
| POST | `/users/{userId}/devices` | Register a device + upload key bundle |
| GET | `/users/{userId}/key-bundle` | Fetch public key bundle |
| PUT | `/users/{userId}/devices/{deviceId}/one-time-prekeys` | Replenish OTPKs |
| POST | `/conversations` | Create a 1:1 conversation |
| POST | `/conversations/{id}/messages` | Send a message |
| GET | `/conversations/{id}/messages` | Get message history |
| POST | `/groups` | Create a group |
| POST | `/groups/{id}/messages` | Send a group message |
| POST | `/groups/{id}/members` | Add a member |
| DELETE | `/groups/{id}/members/{userId}` | Remove a member |
| POST | `/attachments` | Upload a file |
| GET | `/attachments/{id}` | Download a file |

### Real-time

| Method | Path | Description |
|--------|------|-------------|
| GET | `/ws?token={jwt}` | WebSocket connection for live events |

### Platform

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Health check (PostgreSQL + Redis) |
| GET | `/metrics` | Prometheus metrics |

---

## Architecture

```
Browser / Client
    │  HTTP/1.1 + WebSocket
    ▼
nginx (port 3000)
    ├── /api/*, /ws, ... → api:8080 (Rust backend)
    ├── /oidc/           → oidc:80  (dev only mock)
    └── /*               → web:3000 (Next.js App)
    
api (port 8080)  ←→  PostgreSQL  ←→  Redis
```

Data isolation is enforced at the application layer — every database query is scoped to `tenant_id`.

---

## Rebuilding after code changes

```sh
# Rebuild the API image
docker compose build api

# Restart (dev mode)
docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d api

# Restart (production)
docker compose up -d api
```

---

## Stopping and cleanup

```sh
# Stop all services (data volumes preserved)
docker compose down

# Stop and remove all data
docker compose down -v
```

---

## Production checklist

- [ ] Set strong random values for `POSTGRES_PASSWORD`, `REDIS_PASSWORD`, `ADMIN_TOKEN`
- [ ] Put a TLS-terminating reverse proxy (nginx, Caddy, Traefik) in front of the API
- [ ] Register your real OIDC tenant (Step 5b above)
- [ ] Do NOT use `docker-compose.dev.yml` in production
- [ ] Back up the `postgres_data` and `attachments_data` volumes regularly
- [ ] Set `RUST_LOG=warn` to reduce log volume in production
