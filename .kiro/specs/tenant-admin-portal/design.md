# Design Document: Tenant Admin Portal

## Overview

This feature adds a **Next.js 14 web application** and the backing API endpoints to support it. The web app is a single Next.js project covering all three user-facing surfaces:

1. **`/`** — Chat client (existing dev client functionality, ported to React)
2. **`/admin`** — Admin Dashboard for the platform operator
3. **`/register`** — Tenant Self-Registration Portal (public)

**Technology choices:**
- **Next.js 14** with App Router
- **TypeScript** (strict mode)
- **Tailwind CSS** for styling, replicating the existing dark theme palette
- **`next/navigation`** for routing
- **`fetch`** for API calls (no extra HTTP client library)
- **`sessionStorage`** for ephemeral auth tokens (no server-side sessions needed)

The Rust API is unchanged. The Next.js app calls it as a backend API. In Docker, the Next.js app runs as a Node.js container; nginx proxies all traffic through it.

---

## Architecture

```
Browser
    │
    │  All routes (HTTP + WebSocket for /api/ws proxy)
    ▼
nginx (port 3000)
    │
    ├── /api/*       → proxy to api:8080  (Rust backend)
    ├── /ws          → proxy to api:8080  (WebSocket upgrade)
    └── /*           → proxy to web:3000  (Next.js)

web:3000 (Next.js / Node.js)
    ├── app/
    │   ├── page.tsx                → Chat client  (/)
    │   ├── admin/page.tsx          → Admin Dashboard (/admin)
    │   └── register/page.tsx       → Registration Portal (/register)
    └── calls /api/* via server-side fetch or client-side fetch

api:8080 (Rust/Axum — unchanged)
    ├── GET  /admin/tenants
    ├── POST /registrations
    ├── GET  /registrations/{id}
    ├── GET  /admin/registrations
    ├── POST /admin/registrations/{id}/approve
    └── POST /admin/registrations/{id}/reject
```

### nginx proxy update

All API calls from the Next.js app go through nginx with the `/api` prefix stripped:

```nginx
# Forward /api/* to the Rust backend (strips /api prefix)
location /api/ {
    proxy_pass http://api:8080/;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
}

# WebSocket
location /ws {
    proxy_pass http://api:8080;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "Upgrade";
    proxy_read_timeout 3600s;
}

# All other traffic → Next.js
location / {
    proxy_pass http://web:3000;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
}
```

The Next.js app uses `/api/` as the base URL for all backend calls. This avoids CORS entirely — everything is same-origin from the browser's perspective.

---

## Database

### New table: `tenant_registrations`

```sql
CREATE TABLE tenant_registrations (
    registration_id   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    app_name          TEXT NOT NULL,
    oidc_issuer       TEXT NOT NULL,
    contact_email     TEXT NOT NULL,
    status            TEXT NOT NULL DEFAULT 'pending'
                        CHECK (status IN ('pending', 'approved', 'rejected')),
    registration_token TEXT NOT NULL UNIQUE,
    tenant_id         UUID REFERENCES tenants(tenant_id),
    rejection_reason  TEXT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_registrations_status ON tenant_registrations(status);
CREATE INDEX idx_registrations_issuer ON tenant_registrations(oidc_issuer);
```

Token generation: `hex::encode(rand::random::<[u8; 32]>())` — 256 bits stored as 64-char hex.

---

## Backend API Design

### New error codes (add to `common/src/lib.rs`)

```rust
pub const INVALID_OIDC_ISSUER: &str = "invalid_oidc_issuer";
pub const INVALID_EMAIL: &str = "invalid_email";
pub const INVALID_APP_NAME: &str = "invalid_app_name";
pub const ISSUER_ALREADY_REGISTERED: &str = "issuer_already_registered";
pub const INVALID_REGISTRATION_TOKEN: &str = "invalid_registration_token";
pub const REGISTRATION_NOT_PENDING: &str = "registration_not_pending";
```

### `GET /admin/tenants` — list all tenants

**Response 200:**
```json
[{ "tenant_id": "uuid", "name": "Acme", "oidc_issuer": "https://...", "active": true }]
```

### `POST /registrations` — submit registration (public)

**Request:** `{ app_name, oidc_issuer, contact_email }`
**Response 201:** `{ registration_id, registration_token }`

Validation: `app_name` 1–100 chars, `oidc_issuer` starts with `https://`, email contains `@`. Returns 409 on duplicate issuer, 422 on validation failure.

### `GET /registrations/{id}` — check status

Auth: `Authorization: Bearer {registration_token}`

**Response 200:** `{ registration_id, app_name, oidc_issuer, status, created_at, tenant_id, rejection_reason }`

### `GET /admin/registrations?status=pending|approved|rejected`

Auth: `ADMIN_TOKEN`. Returns array of registration objects.

### `POST /admin/registrations/{id}/approve`

Auth: `ADMIN_TOKEN`. Provisions tenant. Returns 200 `{ tenant_id }`.

### `POST /admin/registrations/{id}/reject`

Auth: `ADMIN_TOKEN`. Body: `{ reason? }`. Returns 200 `{}`.

---

## Components and Interfaces

### Next.js project structure

```
web/
├── app/
│   ├── layout.tsx              — root layout, global styles, dark theme
│   ├── page.tsx                — Chat client (/)
│   ├── admin/
│   │   ├── page.tsx            — Admin Dashboard (/admin)
│   │   └── _components/        — admin-only components
│   │       ├── TenantCard.tsx
│   │       ├── RegistrationCard.tsx
│   │       ├── CreateTenantForm.tsx
│   │       └── UsageModal.tsx
│   └── register/
│       ├── page.tsx            — Registration Portal (/register)
│       └── _components/
│           ├── RegisterForm.tsx
│           ├── SuccessView.tsx
│           └── StatusView.tsx
├── components/
│   ├── ui/
│   │   ├── Button.tsx          — primary, ghost, danger-outline variants
│   │   ├── Input.tsx           — with error state
│   │   ├── Badge.tsx           — active/inactive/pending variants
│   │   ├── Card.tsx
│   │   ├── Modal.tsx           — confirmation dialog
│   │   ├── CopyButton.tsx      — copies text to clipboard
│   │   └── Toast.tsx           — success/error notifications
│   └── layout/
│       ├── Header.tsx
│       └── Sidebar.tsx
├── lib/
│   ├── api.ts                  — typed fetch wrappers for all API calls
│   ├── auth.ts                 — sessionStorage token helpers
│   └── types.ts                — TypeScript interfaces for API responses
├── hooks/
│   ├── useAdminAuth.ts         — manages admin token state
│   ├── useTenants.ts           — tenant list fetch + mutations
│   ├── useRegistrations.ts     — registration list fetch + mutations
│   └── useWebSocket.ts         — WebSocket connection for chat
├── tailwind.config.ts
├── next.config.ts
├── tsconfig.json
├── package.json
└── Dockerfile
```

### Key TypeScript interfaces (`lib/types.ts`)

```typescript
export interface Tenant {
  tenant_id: string;
  name: string;
  oidc_issuer: string;
  active: boolean;
}

export interface TenantRegistration {
  registration_id: string;
  app_name: string;
  oidc_issuer: string;
  status: 'pending' | 'approved' | 'rejected';
  created_at: string;
  tenant_id: string | null;
  rejection_reason: string | null;
}

export interface SubmitRegistrationResponse {
  registration_id: string;
  registration_token: string;
}

export interface ApiError {
  error_code: string;
  message: string;
  request_id: string;
}
```

### API client (`lib/api.ts`)

```typescript
const BASE = '/api';  // nginx strips /api → Rust backend

async function apiFetch<T>(
  method: string,
  path: string,
  body?: unknown,
  token?: string
): Promise<T> { ... }

// Typed API methods
export const api = {
  // Admin
  listTenants: (token: string) => apiFetch<Tenant[]>('GET', '/admin/tenants', undefined, token),
  createTenant: (token: string, data: {...}) => apiFetch<{tenant_id: string}>(...),
  deactivateTenant: (token: string, id: string) => apiFetch<void>(...),
  updateOidcIssuer: (token: string, id: string, issuer: string) => apiFetch<void>(...),
  getTenantUsage: (token: string, id: string) => apiFetch<UsageMetrics>(...),
  listRegistrations: (token: string, status?: string) => apiFetch<TenantRegistration[]>(...),
  approveRegistration: (token: string, id: string) => apiFetch<{tenant_id: string}>(...),
  rejectRegistration: (token: string, id: string, reason?: string) => apiFetch<{}>(...),

  // Public
  submitRegistration: (data: {...}) => apiFetch<SubmitRegistrationResponse>(...),
  getRegistrationStatus: (id: string, token: string) => apiFetch<TenantRegistration>(...),
};
```

### Tailwind dark theme config

```typescript
// tailwind.config.ts
const config = {
  theme: {
    extend: {
      colors: {
        bg: '#0f1117',
        surface: '#1a1d27',
        surface2: '#22263a',
        border: '#2e3250',
        accent: '#5c6ef8',
        'accent-dim': '#3d4ab0',
        text: '#e2e4f0',
        'text-dim': '#7b82a8',
        success: '#3ecf8e',
        danger: '#f87171',
        warning: '#fbbf24',
      },
      fontFamily: {
        sans: ['Inter', 'system-ui', 'sans-serif'],
      },
    },
  },
};
```

### Admin page state machine (`app/admin/page.tsx`)

```typescript
type AdminView = 'login' | 'dashboard';

// sessionStorage key: 'adminToken'
// On mount: check sessionStorage → show login or load dashboard
// signOut: clear sessionStorage → set view = 'login'
// Any 401 response: clear token → set view = 'login'
```

### Registration portal state machine (`app/register/page.tsx`)

```typescript
type RegisterView = 'form' | 'success' | 'status';

// sessionStorage keys: 'reg_id', 'reg_token'
// On mount: if reg_id+token in sessionStorage → show status view
// Form submit → success view on 201
// 'Check Status' nav → status view
```

---

## Data Models

### `tenant_registrations` table

| Column | Type | Constraints |
|---|---|---|
| `registration_id` | UUID | PK, DEFAULT gen_random_uuid() |
| `app_name` | TEXT | NOT NULL |
| `oidc_issuer` | TEXT | NOT NULL |
| `contact_email` | TEXT | NOT NULL |
| `status` | TEXT | NOT NULL, DEFAULT 'pending', CHECK IN ('pending','approved','rejected') |
| `registration_token` | TEXT | NOT NULL, UNIQUE |
| `tenant_id` | UUID | REFERENCES tenants(tenant_id), NULL until approved |
| `rejection_reason` | TEXT | NULL unless rejected |
| `created_at` | TIMESTAMPTZ | NOT NULL, DEFAULT NOW() |
| `updated_at` | TIMESTAMPTZ | NOT NULL, DEFAULT NOW() |

### `SubmitRequest`

```typescript
interface SubmitRequest {
  app_name: string;      // 1–100 chars
  oidc_issuer: string;   // must start with "https://"
  contact_email: string; // must contain "@" with non-empty parts
}
```

### `TenantListItem`

```typescript
interface TenantListItem {
  tenant_id: string;
  name: string;
  oidc_issuer: string;
  active: boolean;
}
```

---

## Error Handling

### HTTP status code mapping (Rust backend)

| Condition | Status | `error_code` |
|---|---|---|
| `oidc_issuer` does not start with `https://` | 422 | `invalid_oidc_issuer` |
| `contact_email` malformed | 422 | `invalid_email` |
| `app_name` empty or > 100 chars | 422 | `invalid_app_name` |
| Duplicate `oidc_issuer` | 409 | `issuer_already_registered` |
| Wrong RegistrationToken | 401 | `invalid_registration_token` |
| Registration not `pending` on approve/reject | 409 | `registration_not_pending` |
| Not found | 404 | `not_found` |
| PostgreSQL error | 503 | `storage_unavailable` |

### Frontend error handling (Next.js)

- `useAdminAuth` hook detects 401 responses → clears sessionStorage → redirects to `/admin` login view.
- All API mutations use React state + try/catch → render `<Toast>` component for errors.
- Form validation uses React controlled inputs with per-field error state, validated on submit and on blur.
- The `api.ts` `apiFetch` function throws a typed `ApiError` on non-2xx responses, caught by calling components.

### Validation parity

| Field | Client (TypeScript) | Server (Rust) |
|---|---|---|
| `app_name` | `value.trim().length >= 1 && value.length <= 100` | `!app_name.is_empty() && app_name.len() <= 100` |
| `oidc_issuer` | `value.startsWith('https://')` | `oidc_issuer.starts_with("https://")` |
| `contact_email` | `/^[^@]+@[^@]+\.[^@]+$/.test(value)` | contains `@` with non-empty local and domain |

---

## Testing Strategy

### Unit tests (Rust backend)

- `submit_registration`: field validation rejection, 409 on duplicate issuer, 201 correct shape.
- `get_registration`: 401 on wrong token, 404 on missing ID, correct status shapes.
- `approve_registration`: 409 on non-pending, successful approval provisions tenant.
- `reject_registration`: 409 on non-pending, stores reason.
- `list_tenants`: returns all tenants including inactive.

### Integration tests (Rust backend)

- Full registration flow: submit → pending → approve → approved with tenant_id → tenant visible in list.
- Duplicate issuer: two submissions same issuer → 409.
- Token isolation: two registrations, cross-token access → 401.

### Frontend tests (Next.js)

- Unit tests with **Jest + React Testing Library** for pure components (Button, Input, Badge, CopyButton).
- Integration tests for form submission flows using **MSW (Mock Service Worker)** to intercept API calls.
- E2E tests with **Playwright** covering: admin login → create tenant → deactivate; register → check status pending → check status approved.

---

## Deployment

### `web/Dockerfile`

```dockerfile
FROM node:20-alpine AS builder
WORKDIR /app
COPY package*.json ./
RUN npm ci
COPY . .
RUN npm run build

FROM node:20-alpine AS runtime
WORKDIR /app
ENV NODE_ENV=production
COPY --from=builder /app/.next/standalone ./
COPY --from=builder /app/.next/static ./.next/static
COPY --from=builder /app/public ./public
EXPOSE 3000
CMD ["node", "server.js"]
```

### `next.config.ts`

```typescript
const config: NextConfig = {
  output: 'standalone',  // for Docker
  async rewrites() {
    return []; // nginx handles routing, no Next.js rewrites needed
  },
};
```

### docker-compose addition

```yaml
web:
  build:
    context: ./web
    dockerfile: Dockerfile
  restart: unless-stopped
  environment:
    NODE_ENV: production
  depends_on:
    - api
  networks:
    - internal
    - external
```

---

## Correctness Properties

### Property 1: RegistrationToken Unguessability

`hex::encode(rand::random::<[u8; 32]>())` — 256 bits of cryptographic randomness ensures tokens cannot be predicted or enumerated by brute force.

**Validates: Requirements 8.7**

### Property 2: Registration Status Isolation

For any two RegistrationIDs R1 and R2 with tokens T1 and T2, calling `GET /registrations/R1` with `Authorization: Bearer T2` SHALL return HTTP 401. No registration is readable without its exact token.

**Validates: Requirements 9.2, 9.5**

### Property 3: OIDC Issuer Uniqueness

At most one record across active `tenants` and non-rejected `tenant_registrations` may hold any given `oidc_issuer` value. Duplicates return HTTP 409 `issuer_already_registered`.

**Validates: Requirements 8.3, 3.4**

### Property 4: Approve/Reject Idempotency

Calling approve or reject on a non-pending registration returns HTTP 409 `registration_not_pending` without modifying any stored fields.

**Validates: Requirements 10.5, 10.8**

### Property 5: Client-Side Validation Parity

The TypeScript validation rules in the Next.js app accept exactly the same set of inputs as the Rust server-side rules. Any input rejected client-side is also rejected server-side, and vice versa.

**Validates: Requirements 3.5, 5.4, 11.2**
