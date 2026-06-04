# Tenant Admin Portal (Web UI)

This directory contains the Next.js 14 web application for managing tenants and registrations in the `rust-e2e-chat-api` stack.

## Routes

- **`/admin`** — The Tenant Admin Dashboard. Platform operators can log in using the `ADMIN_TOKEN` to view usage, update configurations, and approve/reject pending tenant registrations.
- **`/register`** — The Self-Registration Portal. Public endpoint where prospective clients can submit their app name and OIDC issuer to request tenant access to the platform.

## Architecture

This is a Next.js 14 App Router project using TypeScript and Tailwind CSS (Dark theme).
It is compiled via the standalone output mode into a multi-stage Docker container for deployment.

The web app relies completely on the Rust API (`crates/api`) for all state and data mutations.

### Nginx Proxy

In both development and production, the Next.js app calls its own `/api/*` routes. The `nginx` container intercepts these calls, strips the `/api` prefix, and forwards the requests to the Rust backend (`api:8080`).

This setup guarantees that the browser considers the API to be same-origin, completely eliminating CORS issues.

## Development

```bash
npm install
npm run dev
```

Open [http://localhost:3000](http://localhost:3000) with your browser to see the result. Note that API calls will fail unless the backend is running and the nginx proxy is routing traffic to it. The recommended way to develop is using Docker Compose.
