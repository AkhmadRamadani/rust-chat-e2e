# Implementation Plan: Tenant Admin Portal

## Overview

Build a Next.js 14 web application (App Router, TypeScript, Tailwind CSS) covering the chat client, admin dashboard, and tenant self-registration portal, backed by new Rust API endpoints for registration management.

---

## Tasks

- [x] 1. Database migration — `tenant_registrations` table
  - Create `migrations/20240601000000_create_tenant_registrations.sql`
  - Columns: `registration_id UUID PK`, `app_name TEXT NOT NULL`, `oidc_issuer TEXT NOT NULL`, `contact_email TEXT NOT NULL`, `status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected'))`, `registration_token TEXT NOT NULL UNIQUE`, `tenant_id UUID REFERENCES tenants(tenant_id)`, `rejection_reason TEXT`, `created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()`, `updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()`
  - Add indexes: `idx_registrations_status ON tenant_registrations(status)`, `idx_registrations_issuer ON tenant_registrations(oidc_issuer)`
  - _Requirements: 10.9_

- [x] 2. Add new error codes to `crates/common/src/lib.rs`
  - Add to `pub mod error_codes`: `INVALID_OIDC_ISSUER`, `INVALID_EMAIL`, `INVALID_APP_NAME`, `ISSUER_ALREADY_REGISTERED`, `INVALID_REGISTRATION_TOKEN`, `REGISTRATION_NOT_PENDING`
  - _Requirements: 3.5, 8.3, 8.4, 8.5, 8.6, 9.2, 10.5, 10.8_

- [x] 3. Implement `GET /admin/tenants` — list all tenants
  - Add `list_tenants` handler to `crates/api/src/admin.rs`
  - SQL: `SELECT tenant_id, name, oidc_issuer, active FROM tenants ORDER BY created_at`
  - Return `Json<Vec<TenantListItem>>` where `TenantListItem { tenant_id, name, oidc_issuer, active }`
  - Add `.get(admin::list_tenants)` to the existing `"/admin/tenants"` route in `build_router`
  - _Requirements: 10.1_

- [x] 4. Implement registration API handlers (`crates/api/src/registrations.rs`)
  - Create `RegistrationState { pool: PgPool, tenant_repo: Arc<dyn TenantRepository> }`
  - `submit_registration` (`POST /registrations`, no auth):
    - Validate `app_name` (1–100 chars), `oidc_issuer` (starts with `https://`), `contact_email` (contains `@`)
    - Check uniqueness of `oidc_issuer` across `tenants` (active) and `tenant_registrations` (pending/approved) — return 409 `issuer_already_registered`
    - Generate token: `hex::encode(rand::random::<[u8; 32]>())`
    - INSERT into `tenant_registrations`, return 201 `{ registration_id, registration_token }`
  - `get_registration` (`GET /registrations/{id}`, Bearer token auth):
    - Fetch by `registration_id`, return 404 if missing
    - Compare token → 401 `invalid_registration_token` on mismatch
    - Return 200 with full registration fields
  - `list_registrations` (`GET /admin/registrations`, AdminAuthLayer):
    - Accept optional `?status=` query param
    - Return array of registration objects (without token field)
  - `approve_registration` (`POST /admin/registrations/{id}/approve`, AdminAuthLayer):
    - Return 404 if missing, 409 `registration_not_pending` if status ≠ `pending`
    - Call `tenant_repo.create_tenant(app_name, oidc_issuer)` to provision
    - UPDATE status to `approved`, store `tenant_id`
    - Return 200 `{ tenant_id }`
  - `reject_registration` (`POST /admin/registrations/{id}/reject`, AdminAuthLayer):
    - Return 404 if missing, 409 `registration_not_pending` if status ≠ `pending`
    - UPDATE status to `rejected`, store optional reason
    - Return 200 `{}`
  - Add `mod registrations;` and `use registrations::*;` to `main.rs`
  - Add `hex = "0.4"` to `crates/api/Cargo.toml` if not present
  - _Requirements: 8.1–8.7, 9.1–9.5, 10.2–10.9_

- [x] 5. Wire registration routes into `build_router` and `main.rs`
  - In `main()`: construct `RegistrationState { pool: pool.clone(), tenant_repo: tenant_repo.clone() }`
  - In `build_router`: add parameter `registration_state: registrations::RegistrationState`
  - Add public routes (no auth): `POST /registrations`, `GET /registrations/:id`
  - Add admin routes (AdminAuthLayer): `GET /admin/registrations`, `POST /admin/registrations/:id/approve`, `POST /admin/registrations/:id/reject`
  - Update `build_router_without_health` test helper with a stub `RegistrationState`
  - _Requirements: 8.1, 10.2_

- [ ] 6. Scaffold Next.js 14 project at `web/`
  - Run: `npx create-next-app@latest web --typescript --tailwind --eslint --app --src-dir=false --import-alias="@/*"`
  - Configure `tailwind.config.ts` with the dark theme color palette:
    - `bg: '#0f1117'`, `surface: '#1a1d27'`, `surface2: '#22263a'`, `border: '#2e3250'`
    - `accent: '#5c6ef8'`, `accent-dim: '#3d4ab0'`, `text: '#e2e4f0'`, `text-dim: '#7b82a8'`
    - `success: '#3ecf8e'`, `danger: '#f87171'`, `warning: '#fbbf24'`
    - Font: `Inter, system-ui, sans-serif`
  - Configure `next.config.ts` with `output: 'standalone'`
  - Create `web/Dockerfile` (multi-stage: builder + runtime using `node:20-alpine`)
  - _Requirements: 13.1, 13.2, 13.4_

- [ ] 7. Create shared UI component library (`web/components/ui/`)
  - `Button.tsx` — variants: `primary`, `ghost`, `danger-outline`, `sm` size modifier
  - `Input.tsx` — controlled input with `label`, `error` string prop, `placeholder`
  - `Badge.tsx` — variants: `active` (green), `inactive` (red), `pending` (yellow)
  - `Card.tsx` — surface2 background, border, rounded corners
  - `Modal.tsx` — confirmation dialog with title, message, confirm/cancel buttons
  - `CopyButton.tsx` — copies text via `navigator.clipboard.writeText`, shows "Copied!" feedback
  - `Toast.tsx` — success/error notification with auto-dismiss
  - All components use Tailwind classes only, no inline styles
  - _Requirements: 13.1, 13.2, 13.5_

- [ ] 8. Create typed API client and auth helpers (`web/lib/`)
  - `web/lib/types.ts`: TypeScript interfaces for `Tenant`, `TenantRegistration`, `UsageMetrics`, `ApiError`, `SubmitRegistrationResponse`
  - `web/lib/api.ts`: typed `apiFetch<T>` wrapper using `/api/` base URL (nginx-proxied); throws `ApiError` on non-2xx; exports `api` object with all endpoint methods
  - `web/lib/auth.ts`: `getAdminToken()`, `setAdminToken(token)`, `clearAdminToken()` — reads/writes `sessionStorage.adminToken`; `getRegistrationCredentials()`, `setRegistrationCredentials(id, token)`, `clearRegistrationCredentials()` — reads/writes `reg_id`/`reg_token`
  - _Requirements: 1.3, 9.8_

- [ ] 9. Implement Admin Dashboard page (`web/app/admin/page.tsx`)
  - Client component (`'use client'`)
  - State machine: `view: 'login' | 'dashboard'`; on mount check `getAdminToken()` → if present set `view='dashboard'`
  - **Login view**: `<Input>` for token + `<Button>` "Sign In"; on submit call `api.listTenants(token)` to validate; store token on success; show error on 401/403
  - **Dashboard view**:
    - Header with pending registrations `<Badge>` count + "Sign Out" `<Button>`
    - Pending Registrations section: `<RegistrationCard>` per item with Approve/Reject
    - Tenants section: `<TenantCard>` per item with Copy ID, View Usage, Update OIDC, Deactivate
    - "Create Tenant" `<CreateTenantForm>` at top of Tenants section
  - Any API call returning 401 → `clearAdminToken()` + set `view='login'` + show error toast
  - _Requirements: 1.1–1.5, 2.1–2.5, 3.1–3.5, 4.1–4.5, 5.1–5.4, 6.1–6.5, 7.1–7.5_

- [ ] 10. Implement admin sub-components (`web/app/admin/_components/`)
  - `TenantCard.tsx` — displays tenant name, ID (with `<CopyButton>`), issuer, `<Badge>`, usage data (expandable), inline OIDC edit form, deactivate action with `<Modal>` confirmation
  - `RegistrationCard.tsx` — displays app name, email, issuer, timestamp, Approve/Reject buttons; Reject opens `<Modal>` with optional reason `<Input>`
  - `CreateTenantForm.tsx` — controlled form with name + issuer fields; client-side validation; calls `api.createTenant`; shows `<Toast>` on success/error
  - `UsageModal.tsx` — modal showing user_count, device_count, message_count_30d, active_wt_sessions as labeled stats
  - _Requirements: 2.1–2.5, 3.1–3.5, 6.1–6.5, 7.1–7.5_

- [ ] 11. Implement Self-Registration Portal page (`web/app/register/page.tsx`)
  - Client component (`'use client'`)
  - State machine: `view: 'form' | 'success' | 'status'`; on mount check `getRegistrationCredentials()` → if present set `view='status'`
  - **Form view**: App Name, OIDC Issuer URL, Contact Email `<Input>` fields with per-field inline error display; on submit validate then call `api.submitRegistration`; save credentials to sessionStorage on 201 response
  - **Success view**: RegistrationID and RegistrationToken displayed in monospace boxes each with `<CopyButton>`; prominent "Save your token" warning; "Check Status →" button
  - **Status view**: RegistrationID + Token inputs (auto-filled from sessionStorage); "Check" button calls `api.getRegistrationStatus`; renders pending/approved/rejected states; approved state shows Tenant ID with `<CopyButton>`
  - _Requirements: 11.1–11.8, 12.1–12.5_

- [ ] 12. Update nginx configuration for Next.js
  - Update `client/nginx.conf` (production) and `client/nginx.dev.conf` (dev):
    - Add `location /api/` block: proxy to `http://api:8080/` (strips `/api` prefix)
    - Update `/ws` block to proxy to `http://api:8080` with WebSocket headers
    - Add `/registrations` to the existing API proxy regex (or route via `/api/registrations`)
    - Add catch-all `location /` block: proxy to `http://web:3000`
    - Remove static file blocks for `/admin-ui/` and `/register/` (Next.js handles routing)
  - _Requirements: 14.1, 14.2, 14.3, 14.4_

- [ ] 13. Add `web` service to docker-compose
  - Add to `docker-compose.yml`:
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
  - Update `client` service depends_on to also include `web`
  - _Requirements: 14.4_

- [ ] 14. Rebuild and verify
  - `docker compose build api web`
  - `docker compose up -d --force-recreate api web migrate client`
  - Verify: `curl http://localhost:3000/` → 200 (Next.js chat page)
  - Verify: `curl http://localhost:3000/admin` → 200 (Admin Dashboard)
  - Verify: `curl http://localhost:3000/register` → 200 (Registration Portal)
  - Verify: `curl -X POST http://localhost:3000/api/registrations -H 'Content-Type: application/json' -d '{"app_name":"Test","oidc_issuer":"https://test.example.com","contact_email":"a@b.com"}'` → 201
  - Verify: `curl http://localhost:3000/api/admin/tenants -H 'Authorization: Bearer {ADMIN_TOKEN}'` → 200 array
  - Manual: submit registration via portal → approve via admin dashboard → confirm provisioned tenant_id shown

---

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1"] },
    { "id": 1, "tasks": ["2", "6"] },
    { "id": 2, "tasks": ["3", "4", "7", "8"] },
    { "id": 3, "tasks": ["5", "9", "11", "12"] },
    { "id": 4, "tasks": ["10", "13"] },
    { "id": 5, "tasks": ["14"] }
  ]
}
```

---

## Notes

- **Next.js version**: 14 with App Router. All pages that use browser APIs (`sessionStorage`, `navigator.clipboard`) must be Client Components (`'use client'`). Static/server pages should remain Server Components for performance.
- **API base URL**: The Next.js app calls `/api/` (relative URL). nginx strips the `/api` prefix and forwards to `api:8080`. This avoids CORS entirely.
- **Auth tokens in sessionStorage**: Both the admin token and registration token are stored in `sessionStorage` (not `localStorage`) — they are cleared when the browser tab is closed.
- **Tailwind dark theme**: The color palette defined in `tailwind.config.ts` matches the existing `client/index.html` CSS variables exactly. Use Tailwind class names (e.g., `bg-surface`, `text-accent`) instead of inline styles or CSS modules.
- **Token generation (Rust)**: `hex::encode(rand::random::<[u8; 32]>())` — the `hex` crate is already a workspace dependency. Confirm it is listed in `crates/api/Cargo.toml`.
- **Approval reuses existing tenant provisioning**: `approve_registration` calls `TenantRepository::create_tenant` — this automatically updates the in-memory `TenantRegistry` and handles all multi-tenancy guarantees.
- **Validation parity**: TypeScript client-side validation rules must match the Rust server-side rules exactly (see Property 5). Any change to one must be reflected in the other.
- **Existing vanilla client**: `client/index.html` is unchanged. The Next.js app is a separate service. Eventually the chat functionality in `client/index.html` can be ported to `web/app/page.tsx`, but that is out of scope for this spec.
