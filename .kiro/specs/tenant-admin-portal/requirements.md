# Requirements Document

## Introduction

The Tenant Admin Portal extends the rust-e2e-chat-api platform with two web-based user interfaces:

1. **Admin Dashboard** — a browser UI for the platform operator to manage all tenants through the existing Admin API. It replaces ad-hoc curl/CLI workflows with a visual interface for creating tenants, deactivating tenants, updating OIDC configuration, and viewing per-tenant usage metrics. Access is gated by the same `ADMIN_TOKEN` already used by the existing `/admin/*` API endpoints.

2. **Tenant Self-Registration Portal** — a public-facing web page where prospective tenants can submit a registration request (app name + OIDC issuer URL). Registrations enter a pending state awaiting operator approval, after which the tenant is provisioned and the registrant receives their Tenant ID and usage dashboard. Both UIs are delivered as static vanilla HTML/CSS/JS files (no build step, no framework) served by the existing nginx container and styled to match the dark theme of `client/index.html`.

## Glossary

- **Admin Dashboard**: The operator-facing web UI at `/admin-ui/index.html` for managing all platform tenants.
- **Admin_UI**: The client-side Admin Dashboard application.
- **Registration_Portal**: The public-facing self-registration web UI at `/register/index.html`.
- **Operator**: The platform owner who holds the `ADMIN_TOKEN` and can approve/reject tenant registrations.
- **Applicant**: A prospective tenant who submits a self-registration request through the Registration_Portal.
- **TenantRegistration**: A pending registration record containing the applicant's app name, OIDC issuer URL, contact email, and current status (`pending`, `approved`, `rejected`).
- **RegistrationID**: A server-assigned UUID identifying a TenantRegistration.
- **RegistrationToken**: A short-lived, single-use opaque token issued to an Applicant upon submission, used to look up their own registration status and, after approval, their provisioned Tenant ID.
- **Tenant**: An active tenant entity in the existing system, identified by a TenantID.
- **TenantID**: A server-assigned UUID identifying a provisioned Tenant.
- **ADMIN_TOKEN**: The static bearer token defined by the `ADMIN_TOKEN` environment variable, used to authenticate requests to the `/admin/*` API routes and the new registration management endpoints.
- **OIDC_Issuer**: A URL identifying a tenant's OIDC identity provider, used as the `iss` claim in JWTs and to resolve the JWKS endpoint.
- **Usage_Metrics**: Aggregated counts returned by `GET /admin/tenants/{id}/usage`: `user_count`, `device_count`, `message_count_30d`, `active_wt_sessions`.
- **Dark_Theme**: The CSS design system defined in `client/index.html`, using CSS custom properties `--bg`, `--surface`, `--surface2`, `--border`, `--accent`, `--text`, `--text-dim`, `--success`, `--danger`, `--warning`, `--radius`, and the Inter/system-ui font stack.

---

## Requirements

### Requirement 1: Admin Dashboard — Authentication and Access Control

**User Story:** As a platform operator, I want the Admin Dashboard to require my `ADMIN_TOKEN` before displaying any tenant data, so that the management interface is never accessible to unauthorized users.

#### Acceptance Criteria

1. WHEN an Operator navigates to the Admin Dashboard URL without having provided a valid `ADMIN_TOKEN`, THE Admin_UI SHALL display only a login form requesting the token, and SHALL NOT render any tenant data.
2. WHEN an Operator submits the login form with a token, THE Admin_UI SHALL send a test request to `GET /admin/tenants/_health` (or an equivalent admin-gated endpoint) using that token as the `Authorization: Bearer` value; IF the response is HTTP 401 or HTTP 403, THEN THE Admin_UI SHALL display an authentication error and SHALL NOT store the token.
3. WHEN the admin token is accepted, THE Admin_UI SHALL store the token in `sessionStorage` (not `localStorage`) and render the full dashboard without a page reload.
4. THE Admin_UI SHALL include a "Sign out" action that clears the stored token from `sessionStorage` and returns the user to the login form.
5. WHEN a subsequent API call from the Admin_UI receives HTTP 401, THE Admin_UI SHALL clear the stored token and redirect the user to the login form.

---

### Requirement 2: Admin Dashboard — Tenant List

**User Story:** As a platform operator, I want to see all provisioned tenants and their status at a glance, so that I can quickly assess the state of the platform.

#### Acceptance Criteria

1. WHEN the Operator is authenticated and the dashboard loads, THE Admin_UI SHALL call `GET /admin/tenants` and render a list of all tenants, showing each tenant's name, TenantID, OIDC issuer URL, and active/inactive status badge.
2. THE Admin_UI SHALL provide a refresh button that re-fetches the tenant list without a full page reload; the refresh button SHALL be available both after a successful data load and after a failed data load.
3. WHILE a tenant list fetch is in progress, THE Admin_UI SHALL display a loading indicator in the tenant list area.
4. IF the tenant list fetch returns an error, THEN THE Admin_UI SHALL display a human-readable error message and a retry button.
5. THE Admin_UI SHALL visually distinguish active tenants from inactive tenants using the `--success` colour for active badges and the `--danger` colour for inactive badges, consistent with the Dark_Theme.

---

### Requirement 3: Admin Dashboard — Create Tenant

**User Story:** As a platform operator, I want to create a new tenant directly from the dashboard, so that I can provision tenants without using curl.

#### Acceptance Criteria

1. THE Admin_UI SHALL expose a "Create Tenant" form with fields for tenant name and OIDC issuer URL.
2. WHEN the Operator submits the Create Tenant form, THE Admin_UI SHALL send `POST /admin/tenants` with the provided name and OIDC issuer URL and the stored `ADMIN_TOKEN`.
3. IF the `POST /admin/tenants` response is HTTP 201, THEN THE Admin_UI SHALL display the newly created TenantID in a copyable format and refresh the tenant list.
4. IF the `POST /admin/tenants` response is HTTP 409, THEN THE Admin_UI SHALL display an error message stating that the OIDC issuer is already registered to an existing tenant.
5. WHEN the Create Tenant form is submitted, THE Admin_UI SHALL validate that the OIDC issuer field is a non-empty string beginning with `https://` before sending the request; IF validation fails, THEN THE Admin_UI SHALL display an inline field error and SHALL NOT send the request. THE ChatAPI SHALL also enforce the same validation server-side and return HTTP 422 with error code `invalid_oidc_issuer` if the submitted value does not begin with `https://`.

---

### Requirement 4: Admin Dashboard — Deactivate Tenant

**User Story:** As a platform operator, I want to deactivate a tenant from the dashboard, so that I can revoke access for a tenant without running command-line tools.

#### Acceptance Criteria

1. THE Admin_UI SHALL provide a "Deactivate" action for each active tenant in the tenant list.
2. WHEN the Operator activates the Deactivate action for a tenant, THE Admin_UI SHALL display a confirmation dialog naming the tenant before sending any request.
3. WHEN the Operator confirms deactivation, THE Admin_UI SHALL send `DELETE /admin/tenants/{tenantID}` with the stored `ADMIN_TOKEN`.
4. IF the `DELETE /admin/tenants/{tenantID}` response is HTTP 204, THEN THE Admin_UI SHALL update the tenant's status badge to inactive in the list without a full page reload.
5. IF the `DELETE /admin/tenants/{tenantID}` response is HTTP 404, THEN THE Admin_UI SHALL display an error message stating that the tenant was not found.

---

### Requirement 5: Admin Dashboard — Update OIDC Issuer

**User Story:** As a platform operator, I want to update a tenant's OIDC issuer URL from the dashboard, so that I can migrate a tenant to a new identity provider without using the raw API.

#### Acceptance Criteria

1. THE Admin_UI SHALL provide an "Update OIDC" action for each tenant in the list that opens an inline form pre-populated with the tenant's current OIDC issuer URL.
2. WHEN the Operator submits an updated OIDC issuer URL, THE Admin_UI SHALL send `PUT /admin/tenants/{tenantID}/oidc` with the new issuer and the stored `ADMIN_TOKEN`.
3. IF the `PUT /admin/tenants/{tenantID}/oidc` response is HTTP 204, THEN THE Admin_UI SHALL update the displayed OIDC issuer URL for that tenant inline and close the edit form. IF the response is any other status code (including HTTP 409, HTTP 404, or a network error), THE Admin_UI SHALL display a human-readable error message and close the edit form automatically.
4. WHEN the Update OIDC form is submitted, THE Admin_UI SHALL validate that the new OIDC issuer is a non-empty string beginning with `https://`; IF validation fails, THEN THE Admin_UI SHALL display an inline field error and SHALL NOT send the request.

---

### Requirement 6: Admin Dashboard — Tenant Usage Metrics

**User Story:** As a platform operator, I want to view usage metrics for any tenant from the dashboard, so that I can monitor platform activity and plan capacity.

#### Acceptance Criteria

1. THE Admin_UI SHALL provide a "View Usage" action for each tenant that, when activated, calls `GET /admin/tenants/{tenantID}/usage` and displays the returned Usage_Metrics.
2. THE Admin_UI SHALL display usage as labelled numeric values: "Users", "Devices", "Messages (30d)", and "Active Sessions".
3. WHILE a usage fetch is in progress, THE Admin_UI SHALL display a loading indicator in the usage area for that tenant; the Operator MAY dismiss the loading indicator while the fetch continues in the background.
4. IF the usage fetch returns an error, THEN THE Admin_UI SHALL display a human-readable error message next to the affected tenant.
5. THE Admin_UI SHALL allow the Operator to refresh the usage data for a specific tenant without reloading the page.

---

### Requirement 7: Admin Dashboard — Registration Queue

**User Story:** As a platform operator, I want to review and action pending tenant registration requests from the dashboard, so that I can approve or reject self-service signups without accessing the database.

#### Acceptance Criteria

1. THE Admin_UI SHALL display a "Pending Registrations" section that calls `GET /admin/registrations?status=pending` and lists all pending TenantRegistrations with their app name, OIDC issuer URL, contact email, and submission timestamp.
2. THE Admin_UI SHALL provide an "Approve" action for each pending registration that, when confirmed, sends `POST /admin/registrations/{registrationID}/approve` with the stored `ADMIN_TOKEN`; WHEN the approval succeeds with HTTP 200, THE Admin_UI SHALL remove the registration from the pending list and display the provisioned TenantID in a success message.
3. THE Admin_UI SHALL provide a "Reject" action for each pending registration that, when confirmed with an optional reason, calls `POST /admin/registrations/{registrationID}/reject` with the stored `ADMIN_TOKEN` and the optional reason in the request body.
4. WHEN a rejection succeeds with HTTP 200, THE Admin_UI SHALL remove the registration from the pending list.
5. THE Admin_UI SHALL display a badge with the count of pending registrations, updating whenever the list is refreshed.

---

### Requirement 8: Tenant Registration API — Submission

**User Story:** As an Applicant, I want to submit my app name, OIDC issuer URL, and contact email through a web form, so that I can request access to the chat platform without contacting the operator directly.

#### Acceptance Criteria

1. THE ChatAPI SHALL expose `POST /registrations` as a public endpoint (no authentication required) that accepts a JSON body containing `app_name` (string), `oidc_issuer` (string), and `contact_email` (string).
2. WHEN a valid registration request is received, THE ChatAPI SHALL create a TenantRegistration record with status `pending`, assign a RegistrationID, issue a RegistrationToken, and return HTTP 201 with the RegistrationID and RegistrationToken.
3. IF the `oidc_issuer` submitted in a registration request is already registered to an active Tenant or an existing non-rejected TenantRegistration, THEN THE ChatAPI SHALL return HTTP 409 with error code `issuer_already_registered`.
4. THE ChatAPI SHALL validate that `oidc_issuer` begins with `https://`; IF the validation fails, THEN THE ChatAPI SHALL return HTTP 422 with error code `invalid_oidc_issuer`.
5. THE ChatAPI SHALL validate that `contact_email` matches a valid email format (contains `@` with non-empty local and domain parts); IF validation fails, THEN THE ChatAPI SHALL return HTTP 422 with error code `invalid_email`.
6. THE ChatAPI SHALL validate that `app_name` is between 1 and 100 characters; IF validation fails, THEN THE ChatAPI SHALL return HTTP 422 with error code `invalid_app_name`.
7. THE ChatAPI SHALL store the RegistrationToken as a cryptographically random, unguessable value of at least 128 bits of entropy.

---

### Requirement 9: Tenant Registration API — Status Lookup

**User Story:** As an Applicant, I want to check the status of my registration request using my RegistrationToken, so that I know whether I have been approved and can retrieve my Tenant ID.

#### Acceptance Criteria

1. THE ChatAPI SHALL expose `GET /registrations/{registrationID}` as a public endpoint that accepts the RegistrationToken in the `Authorization: Bearer` header and returns all registration fields: registration status, app name, OIDC issuer URL, submission timestamp, provisioned TenantID (when approved), and rejection reason (when rejected and a reason was recorded).
2. IF the RegistrationToken presented does not match the RegistrationID, THEN THE ChatAPI SHALL return HTTP 401 with error code `invalid_registration_token`.
3. WHEN the TenantRegistration status is `approved`, THE ChatAPI SHALL include the provisioned TenantID in the response body.
4. WHEN the TenantRegistration status is `rejected`, THE ChatAPI SHALL include the rejection reason (if one was provided) in the response body.
5. THE ChatAPI SHALL NOT expose any registration data to callers who do not present the correct RegistrationToken for that RegistrationID.

---

### Requirement 10: Tenant Registration API — Admin Management

**User Story:** As a platform operator, I want API endpoints to list, approve, and reject pending registrations, and to list all provisioned tenants, so that the Admin Dashboard can drive the full management workflow.

#### Acceptance Criteria

1. THE ChatAPI SHALL expose `GET /admin/tenants` protected by `ADMIN_TOKEN`, which returns a JSON array of all tenant records, each containing `tenant_id`, `name`, `oidc_issuer`, and `active` fields.
2. THE ChatAPI SHALL expose `GET /admin/registrations` protected by `ADMIN_TOKEN`, which returns all TenantRegistration records. THE ChatAPI SHALL support an optional `status` query parameter (`pending`, `approved`, `rejected`) to filter results.
3. WHEN an Operator calls `POST /admin/registrations/{registrationID}/approve` with a valid `ADMIN_TOKEN`, THE ChatAPI SHALL provision a new Tenant using the registration's `app_name` and `oidc_issuer`, update the TenantRegistration status to `approved`, store the resulting TenantID on the registration record, and return HTTP 200 with the provisioned TenantID.
4. IF the registration referenced by an approve request does not exist, THEN THE ChatAPI SHALL return HTTP 404.
5. IF the registration referenced by an approve request is not in `pending` status, THEN THE ChatAPI SHALL return HTTP 409 with error code `registration_not_pending`.
6. WHEN an Operator calls `POST /admin/registrations/{registrationID}/reject` with a valid `ADMIN_TOKEN` and an optional `reason` string, THE ChatAPI SHALL update the TenantRegistration status to `rejected`, store the reason, and return HTTP 200.
7. IF the `reject` request references a RegistrationID that does not exist, THEN THE ChatAPI SHALL return HTTP 404.
8. IF the `reject` request references a TenantRegistration that is not in `pending` status, THEN THE ChatAPI SHALL return HTTP 409 with error code `registration_not_pending`.
9. THE ChatAPI SHALL store all TenantRegistration records in the PostgreSQL database using a `tenant_registrations` table.

---

### Requirement 11: Self-Registration Portal — Submission Form

**User Story:** As an Applicant, I want a clean, guided web form to submit my registration request, so that I can sign up without reading API documentation.

#### Acceptance Criteria

1. THE Registration_Portal SHALL provide a single-page form with fields for "App Name", "OIDC Issuer URL", and "Contact Email".
2. WHEN the Applicant submits the form, THE Registration_Portal SHALL validate all fields client-side before sending: app name must be non-empty (max 100 chars), OIDC issuer must start with `https://`, email must contain `@`.
3. IF client-side validation fails, THEN THE Registration_Portal SHALL display an inline error below each failing field and SHALL NOT send the request.
4. WHEN the form is submitted and passes client-side validation, THE Registration_Portal SHALL call `POST /registrations` and display a loading indicator while the request is in flight.
5. WHEN the `POST /registrations` response is HTTP 201, THE Registration_Portal SHALL display a success view showing the RegistrationID and RegistrationToken to the Applicant with a prominent instruction to save the token.
6. THE Registration_Portal SHALL provide a "Copy Token" button in the success view that copies the RegistrationToken to the clipboard.
7. IF the `POST /registrations` response is HTTP 409, THEN THE Registration_Portal SHALL display an error message stating that the OIDC issuer is already registered.
8. THE Registration_Portal SHALL persist the RegistrationID and RegistrationToken in `sessionStorage` so the Applicant can check status without re-entering the token after a page refresh within the same browser session.

---

### Requirement 12: Self-Registration Portal — Status and Tenant Dashboard

**User Story:** As an Applicant who has registered, I want to check my registration status and, once approved, see my Tenant ID and usage metrics, so that I can start integrating without waiting for an email.

#### Acceptance Criteria

1. THE Registration_Portal SHALL provide a "Check Status" view where the Applicant can enter their RegistrationID and RegistrationToken to query `GET /registrations/{registrationID}`.
2. WHEN the status response indicates `pending`, THE Registration_Portal SHALL display a "Pending approval" message with the submission timestamp.
3. WHEN the status response indicates `approved`, THE Registration_Portal SHALL display the Tenant ID in a copyable format; THE Registration_Portal SHALL attempt to call `GET /admin/tenants/{tenantID}/usage` using the Applicant's RegistrationToken as the bearer token only when the Applicant explicitly requests usage data; IF this call returns HTTP 401, THE Registration_Portal SHALL display the Tenant ID only without usage metrics and note that usage data requires operator credentials.
4. WHEN the status response indicates `rejected`, THE Registration_Portal SHALL display a "Registration rejected" message and, if a reason was provided, display the reason.
5. THE Registration_Portal SHALL display the Tenant ID in a monospace copyable element with a "Copy" button.

---

### Requirement 13: Visual Design and Theming

**User Story:** As a user of either UI, I want the admin dashboard and registration portal to match the dark theme used in the existing dev client, so that the platform has a consistent visual identity.

#### Acceptance Criteria

1. THE Admin_UI and Registration_Portal SHALL use the same CSS custom property palette defined in `client/index.html`: `--bg: #0f1117`, `--surface: #1a1d27`, `--surface2: #22263a`, `--border: #2e3250`, `--accent: #5c6ef8`, `--text: #e2e4f0`, `--text-dim: #7b82a8`, `--success: #3ecf8e`, `--danger: #f87171`, `--warning: #fbbf24`.
2. THE Admin_UI and Registration_Portal SHALL use Inter or system-ui as the primary typeface, consistent with the `--font` variable in the Dark_Theme.
3. THE Admin_UI and Registration_Portal SHALL be delivered as self-contained HTML files with no external stylesheet or JavaScript dependencies; all CSS and JavaScript SHALL be inlined or included as sibling static files served by nginx.
4. THE Admin_UI and Registration_Portal SHALL be fully usable at viewport widths from 360px to 1920px without horizontal scrolling.
5. THE Admin_UI and Registration_Portal SHALL follow the existing component patterns: `.btn`, `.btn.ghost`, `.btn.danger-outline`, `.form-row`, `.section`, `.badge.active`, `.badge.inactive`, `.log-entry.ok`, `.log-entry.err` and equivalent structural conventions.

---

### Requirement 14: Routing and Static File Serving

**User Story:** As a developer deploying the platform, I want the new UIs to be served by the existing nginx container without changes to the Rust API, so that the deployment footprint stays minimal.

#### Acceptance Criteria

1. THE Admin_UI SHALL be served by nginx at the path `/admin-ui/` as a static HTML file.
2. THE Registration_Portal SHALL be served by nginx at the path `/register/` as a static HTML file.
3. THE nginx configuration SHALL route all requests matching `/registrations` and `/admin/registrations` to the upstream `api:8080` server alongside the existing API route patterns.
4. THE nginx configuration SHALL serve `/admin-ui/` and `/register/` paths from the nginx static file root, consistent with how `client/index.html` is currently served.
5. THE Admin_UI and Registration_Portal HTML files SHALL be placed in the `client/` directory alongside `client/index.html` so that the existing Docker image build and nginx container configuration can serve them without structural changes.

---

## Correctness Properties

### Property A: RegistrationToken Unguessability
THE ChatAPI SHALL generate each RegistrationToken using a cryptographically secure random number generator with at least 128 bits of entropy, such that an adversary cannot predict or enumerate valid tokens by brute force within the expected lifetime of a registration.

**Validates: Requirement 8.7**

### Property B: Registration Status Isolation
FOR ALL distinct RegistrationIDs R1 and R2, a request to `GET /registrations/{R1}` authenticated with the RegistrationToken of R2 SHALL return HTTP 401. No registration record SHALL be accessible to any caller that does not present the correct RegistrationToken for that specific RegistrationID.

**Validates: Requirements 9.2, 9.5**

### Property C: OIDC Issuer Uniqueness
FOR ALL TenantRegistration records and active Tenant records, no two records SHALL share the same `oidc_issuer` value among active tenants and pending/approved registrations. Submitting a registration or creating a tenant with a duplicate issuer SHALL always return HTTP 409.

**Validates: Requirements 8.3, 3.4 (of the base system)**

### Property D: Approval Idempotency — Rejection
WHEN `POST /admin/registrations/{registrationID}/reject` is called on a registration already in `rejected` status, THE ChatAPI SHALL return HTTP 409 with error code `registration_not_pending`, ensuring that a registration cannot be rejected twice and that its stored reason is not overwritten by a second call.

**Validates: Requirement 10.4, 10.5**

### Property E: Client-Side Validation Mirrors Server-Side Validation
FOR ALL form inputs, the client-side validation rules in THE Admin_UI and Registration_Portal SHALL accept exactly the same set of inputs as the corresponding server-side validation rules in THE ChatAPI. Any input rejected client-side SHALL also be rejected server-side, and any input accepted server-side SHALL also pass client-side validation.

**Validates: Requirements 3.5, 5.5, 11.2**
