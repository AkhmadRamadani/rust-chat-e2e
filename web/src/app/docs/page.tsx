'use client';

import React, { useState } from 'react';

// ── Types ─────────────────────────────────────────────────────────────────────

type SectionId =
  | 'overview'
  | 'getting-started'
  | 'tenant-registration'
  | 'authentication'
  | 'device-registration'
  | 'messaging'
  | 'groups'
  | 'attachments'
  | 'websocket'
  | 'errors'
  | 'admin'
  | 'dev-client';

// ── Sub-components ────────────────────────────────────────────────────────────

function Badge({ children, color = 'blue' }: { children: React.ReactNode; color?: 'blue' | 'green' | 'yellow' | 'red' | 'purple' }) {
  const colors = {
    blue:   'bg-[#3d4ab0] text-[#a5aff9]',
    green:  'bg-[#1a4a35] text-[#3ecf8e]',
    yellow: 'bg-[#3d3010] text-[#fbbf24]',
    red:    'bg-[#3d1010] text-[#f87171]',
    purple: 'bg-[#2d1a4a] text-[#c084fc]',
  };
  return (
    <span className={`inline-block text-[11px] font-bold px-2 py-0.5 rounded uppercase tracking-wider font-mono ${colors[color]}`}>
      {children}
    </span>
  );
}

function MethodBadge({ method }: { method: 'GET' | 'POST' | 'PUT' | 'DELETE' }) {
  const colors: Record<string, string> = {
    GET:    'bg-[#1a3d4a] text-[#38bdf8]',
    POST:   'bg-[#1a4a35] text-[#3ecf8e]',
    PUT:    'bg-[#3d3010] text-[#fbbf24]',
    DELETE: 'bg-[#3d1010] text-[#f87171]',
  };
  return (
    <span className={`inline-block text-[11px] font-bold px-2 py-0.5 rounded font-mono w-[56px] text-center ${colors[method]}`}>
      {method}
    </span>
  );
}

function Code({ children }: { children: React.ReactNode }) {
  return (
    <code className="bg-[#22263a] text-[#a5aff9] px-1.5 py-0.5 rounded text-[13px] font-mono">
      {children}
    </code>
  );
}

function CodeBlock({ children, lang = 'json' }: { children: string; lang?: string }) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    navigator.clipboard.writeText(children.trim());
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };
  return (
    <div className="relative group my-3">
      <div className="flex items-center justify-between bg-[#161926] border border-[#2e3250] rounded-t-lg px-4 py-1.5">
        <span className="text-[11px] text-[#7b82a8] font-mono uppercase tracking-wider">{lang}</span>
        <button
          onClick={copy}
          className="text-[11px] text-[#7b82a8] hover:text-[#e2e4f0] transition-colors"
        >
          {copied ? '✓ Copied' : 'Copy'}
        </button>
      </div>
      <pre className="bg-[#0c0e16] border border-t-0 border-[#2e3250] rounded-b-lg px-4 py-3 overflow-x-auto text-[13px] leading-relaxed">
        <code className="text-[#e2e4f0] font-mono whitespace-pre">{children.trim()}</code>
      </pre>
    </div>
  );
}

function RouteRow({ method, path, auth, desc }: { method: 'GET' | 'POST' | 'PUT' | 'DELETE'; path: string; auth: string; desc: string }) {
  return (
    <tr className="border-b border-[#2e3250] hover:bg-[#1e2235] transition-colors">
      <td className="py-2.5 px-3"><MethodBadge method={method} /></td>
      <td className="py-2.5 px-3 font-mono text-[13px] text-[#a5aff9]">{path}</td>
      <td className="py-2.5 px-3 text-[12px] text-[#7b82a8]">{auth}</td>
      <td className="py-2.5 px-3 text-[13px] text-[#c9cce0]">{desc}</td>
    </tr>
  );
}

function SectionHeading({ id, children }: { id: string; children: React.ReactNode }) {
  return (
    <h2
      id={id}
      className="text-[22px] font-bold text-white mb-1 pt-2 flex items-center gap-2 scroll-mt-20"
    >
      {children}
    </h2>
  );
}

function SubHeading({ children }: { children: React.ReactNode }) {
  return (
    <h3 className="text-[16px] font-semibold text-white mt-6 mb-3 flex items-center gap-2">
      {children}
    </h3>
  );
}

function Callout({ type, children }: { type: 'info' | 'warning' | 'tip'; children: React.ReactNode }) {
  const styles = {
    info:    { border: 'border-[#5c6ef8]', bg: 'bg-[#1a1d3d]', icon: 'ℹ️', label: 'Note' },
    warning: { border: 'border-[#fbbf24]', bg: 'bg-[#2d2410]', icon: '⚠️', label: 'Warning' },
    tip:     { border: 'border-[#3ecf8e]', bg: 'bg-[#0d2d22]', icon: '💡', label: 'Tip' },
  };
  const s = styles[type];
  return (
    <div className={`border-l-2 ${s.border} ${s.bg} rounded-r-lg px-4 py-3 my-4 text-[13px] leading-relaxed text-[#c9cce0]`}>
      <span className="font-bold text-white mr-1">{s.icon} {s.label}:</span> {children}
    </div>
  );
}

function Divider() {
  return <div className="border-t border-[#2e3250] my-8" />;
}

// ── Nav items ─────────────────────────────────────────────────────────────────

const NAV: { id: SectionId; label: string; icon: string }[] = [
  { id: 'overview',            label: 'Overview',            icon: '🏠' },
  { id: 'getting-started',     label: 'Getting Started',     icon: '🚀' },
  { id: 'tenant-registration', label: 'Tenant Registration', icon: '🏢' },
  { id: 'authentication',      label: 'Authentication',      icon: '🔐' },
  { id: 'device-registration', label: 'Devices & Keys',      icon: '📱' },
  { id: 'messaging',           label: '1:1 Messaging',       icon: '💬' },
  { id: 'groups',              label: 'Group Messaging',     icon: '👥' },
  { id: 'attachments',         label: 'Attachments',         icon: '📎' },
  { id: 'websocket',           label: 'WebSocket (RT)',       icon: '⚡' },
  { id: 'errors',              label: 'Error Reference',     icon: '🔴' },
  { id: 'admin',               label: 'Admin API',           icon: '🛠️' },
  { id: 'dev-client',          label: 'Dev Client Guide',    icon: '🖥️' },
];

// ── Page ──────────────────────────────────────────────────────────────────────

export default function DocsPage() {
  const [active, setActive] = useState<SectionId>('overview');

  const scrollTo = (id: SectionId) => {
    setActive(id);
    document.getElementById(id)?.scrollIntoView({ behavior: 'smooth' });
  };

  return (
    <div className="flex flex-col h-[100dvh] w-full text-[#e2e4f0] bg-[#0f1117]" style={{ fontFamily: "'Inter', system-ui, sans-serif" }}>

      {/* ── Top bar ──────────────────────────────────────────────────────────── */}
      <header className="flex items-center gap-3 px-5 h-[52px] bg-[#1a1d27] border-b border-[#2e3250] shrink-0 sticky top-0 z-20">
        <span className="text-[15px] font-bold text-white">rust-e2e-chat</span>
        <span className="text-[10px] bg-[#3d4ab0] text-[#a5aff9] px-2 py-0.5 rounded-full font-semibold uppercase tracking-wide">Docs</span>
        <div className="flex-1" />
        <a href="/test" className="text-[13px] text-[#7b82a8] hover:text-[#e2e4f0] transition-colors">← Dev Client</a>
      </header>

      <div className="flex flex-1 overflow-hidden">

        {/* ── Sidebar nav ────────────────────────────────────────────────────── */}
        <nav className="w-[220px] shrink-0 bg-[#1a1d27] border-r border-[#2e3250] overflow-y-auto py-4 hidden lg:block">
          {NAV.map(n => (
            <button
              key={n.id}
              onClick={() => scrollTo(n.id)}
              className={`w-full text-left flex items-center gap-2.5 px-4 py-2 text-[13px] transition-colors ${
                active === n.id
                  ? 'text-[#5c6ef8] bg-[#22263a] font-medium'
                  : 'text-[#7b82a8] hover:text-[#e2e4f0] hover:bg-[#1e2235]'
              }`}
            >
              <span>{n.icon}</span>
              <span>{n.label}</span>
            </button>
          ))}
        </nav>

        {/* ── Content ────────────────────────────────────────────────────────── */}
        <main className="flex-1 overflow-y-auto px-6 lg:px-10 py-8 max-w-4xl mx-auto w-full">

          {/* ── Overview ─────────────────────────────────────────────────────── */}
          <section id="overview">
            <SectionHeading id="overview">🏠 Overview</SectionHeading>
            <p className="text-[#7b82a8] text-[15px] leading-relaxed mb-4">
              <strong className="text-white">rust-e2e-chat</strong> is a multi-tenant, end-to-end encrypted real-time chat platform built in Rust.
              It implements the <strong className="text-white">Signal Protocol</strong> (X3DH + Double Ratchet for 1:1; Sender Keys for groups),
              providing strong cryptographic guarantees while exposing a simple REST + WebSocket API.
            </p>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-3 mb-4">
              {[
                ['Multi-tenant', 'Full data isolation per tenant — one deployment for many apps.'],
                ['End-to-End Encrypted', 'Signal Protocol: X3DH key exchange + Double Ratchet for 1:1; Sender Keys for groups.'],
                ['Real-time WebSocket', 'Live message delivery with offline queue fallback.'],
                ['OIDC Auth', 'Bring your own IdP: Auth0, Keycloak, Okta, Cognito, Firebase, Supabase.'],
                ['File Attachments', 'Upload files up to 100 MB; tenant-isolated storage.'],
                ['REST API', 'HTTP/1.1 JSON API — works from any language or platform.'],
              ].map(([title, desc]) => (
                <div key={title} className="bg-[#1a1d27] border border-[#2e3250] rounded-xl p-4">
                  <div className="font-semibold text-white text-[13px] mb-1">{title}</div>
                  <div className="text-[#7b82a8] text-[13px] leading-relaxed">{desc}</div>
                </div>
              ))}
            </div>

            <SubHeading>Base URL</SubHeading>
            <p className="text-[#7b82a8] text-[13px] mb-2">All API requests are made to the base URL. In local dev, the Nginx proxy handles everything at:</p>
            <CodeBlock lang="text">http://localhost:3000/api</CodeBlock>
            <p className="text-[#7b82a8] text-[13px]">In production, replace with your own domain. The <Code>/api</Code> prefix is stripped by Nginx before forwarding to the Rust backend.</p>
          </section>

          <Divider />

          {/* ── Getting Started ───────────────────────────────────────────────── */}
          <section id="getting-started">
            <SectionHeading id="getting-started">🚀 Getting Started</SectionHeading>

            <SubHeading>Prerequisites</SubHeading>
            <ul className="list-disc list-inside text-[#c9cce0] space-y-1 text-[13px] mb-4">
              <li>Docker Engine ≥ 24</li>
              <li>Docker Compose v2 (<Code>docker compose</Code>, not <Code>docker-compose</Code>)</li>
              <li>Git</li>
            </ul>

            <SubHeading>1. Clone & Configure</SubHeading>
            <CodeBlock lang="bash">{`git clone <your-repo-url>
cd rust-chat
cp .env.example .env`}</CodeBlock>
            <p className="text-[#7b82a8] text-[13px] mb-2">Open <Code>.env</Code> and set the required values:</p>
            <CodeBlock lang="env">{`POSTGRES_PASSWORD=change-me-strong-password
REDIS_PASSWORD=change-me-redis-password
ADMIN_TOKEN=change-me-admin-token   # generate: openssl rand -hex 32`}</CodeBlock>

            <SubHeading>2. Start the Stack</SubHeading>
            <p className="text-[#7b82a8] text-[13px] mb-2">
              <Badge color="blue">Dev Mode</Badge>{' '}
              Includes a mock OIDC server — no external identity provider needed. Perfect for local testing.
            </p>
            <CodeBlock lang="bash">docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d</CodeBlock>
            <p className="text-[#7b82a8] text-[13px] mb-2 mt-3">
              <Badge color="green">Production</Badge>{' '}
              Uses your real OIDC provider. Register it via the Admin API after starting.
            </p>
            <CodeBlock lang="bash">docker compose up -d</CodeBlock>

            <SubHeading>3. Verify Health</SubHeading>
            <CodeBlock lang="bash">curl http://localhost:3000/api/health</CodeBlock>
            <CodeBlock lang="json">{`{"status":"ok"}`}</CodeBlock>
          </section>

          <Divider />

          {/* ── Tenant Registration ───────────────────────────────────────────── */}
          <section id="tenant-registration">
            <SectionHeading id="tenant-registration">🏢 Tenant Registration</SectionHeading>
            <p className="text-[#7b82a8] text-[13px] leading-relaxed mb-4">
              Every application using this platform is a <strong className="text-white">tenant</strong>.
              There are two ways to get a tenant provisioned: the <strong className="text-white">Admin API</strong> (instant, for operators)
              or the <strong className="text-white">Self-Registration Portal</strong> (public, awaits admin approval).
            </p>

            <SubHeading>Option A — Admin creates tenant directly</SubHeading>
            <Callout type="info">This requires the <Code>ADMIN_TOKEN</Code> you set in <Code>.env</Code>. Use this for your own apps or trusted integrations.</Callout>
            <CodeBlock lang="bash">{`curl -X POST http://localhost:3000/api/admin/tenants \\
  -H "Authorization: Bearer YOUR_ADMIN_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{
    "name": "My Chat App",
    "oidc_issuer": "https://your-idp.example.com"
  }'`}</CodeBlock>
            <CodeBlock lang="json">{`{
  "tenant_id": "a1b2c3d4-...",
  "name": "My Chat App",
  "oidc_issuer": "https://your-idp.example.com",
  "active": true
}`}</CodeBlock>
            <p className="text-[#7b82a8] text-[13px] mb-4">
              Save the returned <Code>tenant_id</Code>. Your OIDC JWTs must include a <Code>tid</Code> claim matching this value.
            </p>

            <SubHeading>Option B — Self-Registration (public portal)</SubHeading>
            <p className="text-[#7b82a8] text-[13px] mb-3 leading-relaxed">
              Any applicant can submit a registration request. The platform admin then approves or rejects it via the Admin Portal or API.
            </p>

            <p className="text-[#7b82a8] text-[13px] mb-2 font-medium">Step 1 — Submit a registration request</p>
            <CodeBlock lang="bash">{`curl -X POST http://localhost:3000/api/registrations \\
  -H "Content-Type: application/json" \\
  -d '{
    "app_name": "My Chat App",
    "oidc_issuer": "https://my-idp.example.com",
    "contact_email": "admin@myapp.com"
  }'`}</CodeBlock>
            <CodeBlock lang="json">{`{
  "registration_id": "550e8400-...",
  "registration_token": "reg_tok_abc123..."
}`}</CodeBlock>
            <Callout type="warning">Store both the <Code>registration_id</Code> and <Code>registration_token</Code> — the token is shown only once and is required to check your status.</Callout>

            <p className="text-[#7b82a8] text-[13px] mb-2 font-medium mt-5">Step 2 — Poll registration status</p>
            <CodeBlock lang="bash">{`curl http://localhost:3000/api/registrations/550e8400-... \\
  -H "Authorization: Bearer reg_tok_abc123..."`}</CodeBlock>
            <CodeBlock lang="json">{`{
  "registration_id": "550e8400-...",
  "app_name": "My Chat App",
  "oidc_issuer": "https://my-idp.example.com",
  "contact_email": "admin@myapp.com",
  "status": "pending",   // "pending" | "approved" | "rejected"
  "created_at": "2024-01-01T00:00:00Z",
  "tenant_id": null,     // populated once approved
  "rejection_reason": null
}`}</CodeBlock>

            <SubHeading>Supported OIDC Providers</SubHeading>
            <div className="overflow-x-auto">
              <table className="w-full text-left text-[13px] border-collapse">
                <thead>
                  <tr className="border-b border-[#2e3250] text-[#7b82a8] text-[12px]">
                    <th className="py-2 px-3 font-medium">Provider</th>
                    <th className="py-2 px-3 font-medium">Issuer URL format</th>
                  </tr>
                </thead>
                <tbody className="text-[#c9cce0]">
                  {[
                    ['Auth0', 'https://{your-domain}.auth0.com'],
                    ['Keycloak', 'https://keycloak.example.com/realms/{realm}'],
                    ['Okta', 'https://{your-domain}.okta.com'],
                    ['AWS Cognito', 'https://cognito-idp.{region}.amazonaws.com/{pool-id}'],
                    ['Firebase Auth', 'https://securetoken.google.com/{project-id}'],
                    ['Supabase', 'https://{project-ref}.supabase.co/auth/v1'],
                    ['Mock OIDC (dev)', 'http://oidc/default'],
                  ].map(([p, u]) => (
                    <tr key={p} className="border-b border-[#2e3250] hover:bg-[#1e2235] transition-colors">
                      <td className="py-2 px-3 font-medium text-white">{p}</td>
                      <td className="py-2 px-3 font-mono text-[12px] text-[#a5aff9]">{u}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>

          <Divider />

          {/* ── Authentication ────────────────────────────────────────────────── */}
          <section id="authentication">
            <SectionHeading id="authentication">🔐 Authentication</SectionHeading>
            <p className="text-[#7b82a8] text-[13px] leading-relaxed mb-4">
              All user-facing endpoints require a <Code>Bearer</Code> JWT issued by your OIDC provider.
              The server validates the token against <Code>{'{oidc_issuer}'}/.well-known/jwks.json</Code> and resolves the tenant from the JWT's <Code>tid</Code> claim.
            </p>

            <SubHeading>Required JWT Claims</SubHeading>
            <div className="overflow-x-auto">
              <table className="w-full text-left text-[13px] border-collapse">
                <thead>
                  <tr className="border-b border-[#2e3250] text-[#7b82a8] text-[12px]">
                    <th className="py-2 px-3 font-medium">Claim</th>
                    <th className="py-2 px-3 font-medium">Description</th>
                    <th className="py-2 px-3 font-medium">Example</th>
                  </tr>
                </thead>
                <tbody className="text-[#c9cce0]">
                  {[
                    ['sub', 'User ID (unique per tenant)', '"alice"'],
                    ['tid', 'Tenant ID (UUID matching your registration)', '"a1b2c3d4-..."'],
                    ['iss', 'Issuer URL matching your registered OIDC issuer', '"https://my-idp.example.com"'],
                    ['exp', 'Token expiration (standard)', '1700000000'],
                  ].map(([claim, desc, ex]) => (
                    <tr key={claim} className="border-b border-[#2e3250] hover:bg-[#1e2235] transition-colors">
                      <td className="py-2 px-3 font-mono text-[#a5aff9]">{claim}</td>
                      <td className="py-2 px-3">{desc}</td>
                      <td className="py-2 px-3 font-mono text-[12px] text-[#7b82a8]">{ex}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            <SubHeading>Using the Token</SubHeading>
            <CodeBlock lang="bash">{`curl http://localhost:3000/api/users/alice/key-bundle \\
  -H "Authorization: Bearer eyJ..."`}</CodeBlock>

            <SubHeading>Token Refresh</SubHeading>
            <p className="text-[#7b82a8] text-[13px] mb-2">Exchange an expired access token for a new one using a refresh token:</p>
            <CodeBlock lang="bash">{`curl -X POST http://localhost:3000/api/auth/refresh \\
  -H "Content-Type: application/json" \\
  -d '{"refresh_token": "..."}'`}</CodeBlock>
          </section>

          <Divider />

          {/* ── Device Registration ───────────────────────────────────────────── */}
          <section id="device-registration">
            <SectionHeading id="device-registration">📱 Devices & Key Distribution (KDS)</SectionHeading>
            <p className="text-[#7b82a8] text-[13px] leading-relaxed mb-4">
              Before a user can send or receive encrypted messages, they must register a <strong className="text-white">device</strong> and upload their
              cryptographic key material. This is the Key Distribution Server (KDS) layer, implementing the Signal Protocol key bundle structure.
            </p>

            <Callout type="info">
              Each user can have up to <strong>5 devices</strong>. Each device maintains its own independent cryptographic state.
            </Callout>

            <SubHeading>Register a Device</SubHeading>
            <p className="text-[#7b82a8] text-[13px] mb-2 leading-relaxed">
              Upload your device's key bundle. The server verifies the <Code>signed_prekey_sig</Code> against your <Code>identity_key</Code> before accepting.
            </p>
            <CodeBlock lang="bash">{`curl -X POST http://localhost:3000/api/users/{userId}/devices \\
  -H "Authorization: Bearer eyJ..." \\
  -H "Content-Type: application/json" \\
  -d '{
    "identity_key":      "<base64 Curve25519 public key>",
    "signed_prekey_id":  1,
    "signed_prekey":     "<base64 Curve25519 SPK>",
    "signed_prekey_sig": "<base64 Ed25519 signature>",
    "one_time_prekeys": [
      { "id": 1, "key": "<base64 OTPK>" },
      { "id": 2, "key": "<base64 OTPK>" }
    ]
  }'`}</CodeBlock>
            <CodeBlock lang="json">{`{
  "device_id": "f47ac10b-..."
}`}</CodeBlock>
            <p className="text-[#7b82a8] text-[13px] mt-2">Save the <Code>device_id</Code> — it is required for all subsequent API calls and WebSocket connections.</p>

            <SubHeading>Fetch a Key Bundle (for initiating X3DH)</SubHeading>
            <p className="text-[#7b82a8] text-[13px] mb-2 leading-relaxed">
              Retrieve another user's public key bundle to initiate an X3DH key exchange. This atomically consumes one of their One-Time Pre-Keys (OTPKs).
            </p>
            <CodeBlock lang="bash">{`curl http://localhost:3000/api/users/{recipientUserId}/key-bundle \\
  -H "Authorization: Bearer eyJ..."`}</CodeBlock>
            <CodeBlock lang="json">{`{
  "device_id":        "...",
  "identity_key":     "<base64>",
  "signed_prekey_id": 1,
  "signed_prekey":    "<base64>",
  "signed_prekey_sig":"<base64>",
  "one_time_prekey":  { "id": 5, "key": "<base64>" }
  // null if OTPK pool is depleted — check x-otpk-warning: depleted header
}`}</CodeBlock>
            <Callout type="warning">
              Watch for the <Code>x-otpk-warning: depleted</Code> response header. When present, the recipient has no OTPKs left.
              You should notify your user to replenish their key pool. The server also delivers a real-time <Code>low_otpk</Code> event when the pool drops below 10 keys.
            </Callout>

            <SubHeading>Replenish One-Time Pre-Keys</SubHeading>
            <CodeBlock lang="bash">{`curl -X PUT http://localhost:3000/api/users/{userId}/devices/{deviceId}/one-time-prekeys \\
  -H "Authorization: Bearer eyJ..." \\
  -H "Content-Type: application/json" \\
  -d '{
    "one_time_prekeys": [
      { "id": 50, "key": "<base64>" },
      { "id": 51, "key": "<base64>" }
    ]
  }'`}</CodeBlock>
            <CodeBlock lang="json">{`{ "total_count": 52 }`}</CodeBlock>

            <SubHeading>Rotate Signed Pre-Key</SubHeading>
            <p className="text-[#7b82a8] text-[13px] mb-2">Periodically rotate your Signed Pre-Key for forward secrecy. The new signature is verified before the old key is replaced.</p>
            <CodeBlock lang="bash">{`curl -X PUT http://localhost:3000/api/users/{userId}/devices/{deviceId}/signed-prekey \\
  -H "Authorization: Bearer eyJ..." \\
  -H "Content-Type: application/json" \\
  -d '{
    "signed_prekey_id":  2,
    "signed_prekey":     "<base64 new SPK>",
    "signed_prekey_sig": "<base64 new Ed25519 sig>"
  }'`}</CodeBlock>
          </section>

          <Divider />

          {/* ── Messaging ─────────────────────────────────────────────────────── */}
          <section id="messaging">
            <SectionHeading id="messaging">💬 1:1 Messaging</SectionHeading>
            <p className="text-[#7b82a8] text-[13px] leading-relaxed mb-4">
              1:1 conversations use the <strong className="text-white">X3DH</strong> key agreement protocol for the initial message,
              then switch to the <strong className="text-white">Double Ratchet</strong> algorithm for subsequent messages.
              All encryption happens on the client — the server only stores opaque ciphertext.
            </p>

            <SubHeading>Start a Conversation (X3DH Init)</SubHeading>
            <p className="text-[#7b82a8] text-[13px] mb-2 leading-relaxed">
              The first message must be a <Code>x3dh_init</Code> envelope. Fetch the recipient's key bundle first (see Devices & Keys),
              compute the X3DH shared secret client-side, then encrypt and send:
            </p>
            <CodeBlock lang="bash">{`curl -X POST http://localhost:3000/api/conversations \\
  -H "Authorization: Bearer eyJ..." \\
  -H "Content-Type: application/json" \\
  -d '{
    "recipient_user_id":   "bob",
    "recipient_device_id": "f47ac10b-...",
    "envelope": {
      "conversation_id":   "00000000-0000-0000-0000-000000000000",
      "ciphertext":        "<base64 encrypted payload>",
      "protocol_header": {
        "type": "x3dh_init",
        "ek":   "<base64 ephemeral key>",
        "spk_id": 1,
        "otpk_id": 5
      }
    }
  }'`}</CodeBlock>
            <CodeBlock lang="json">{`{
  "conversation_id": "9f9c4e2a-..."
}`}</CodeBlock>
            <p className="text-[#7b82a8] text-[13px] mt-2">Save the <Code>conversation_id</Code> for all subsequent messages in this thread.</p>

            <SubHeading>Send a Message (Double Ratchet)</SubHeading>
            <p className="text-[#7b82a8] text-[13px] mb-2">All subsequent messages in an established conversation use a <Code>double_ratchet</Code> header:</p>
            <CodeBlock lang="bash">{`curl -X POST http://localhost:3000/api/conversations/{conversationId}/messages \\
  -H "Authorization: Bearer eyJ..." \\
  -H "Content-Type: application/json" \\
  -d '{
    "envelope": {
      "conversation_id": "9f9c4e2a-...",
      "ciphertext": "<base64>",
      "protocol_header": {
        "type": "double_ratchet",
        "dh":  "<base64 ratchet public key>",
        "n":   42,
        "pn":  0
      },
      "attachment_id": null
    }
  }'`}</CodeBlock>
            <CodeBlock lang="json">{`{
  "seq":       43,
  "server_ts": 1700000000000
}`}</CodeBlock>

            <SubHeading>Fetch Message History</SubHeading>
            <CodeBlock lang="bash">{`curl "http://localhost:3000/api/conversations/{conversationId}/messages?limit=50&before_seq=100" \\
  -H "Authorization: Bearer eyJ..."`}</CodeBlock>
            <CodeBlock lang="json">{`{
  "messages": [
    {
      "seq":               43,
      "sender_user_id":    "alice",
      "sender_device_id":  "...",
      "ciphertext":        "<base64>",
      "protocol_header":   { "type": "double_ratchet", ... },
      "server_ts":         1700000000000,
      "attachment_id":     null
    }
  ]
}`}</CodeBlock>
          </section>

          <Divider />

          {/* ── Groups ───────────────────────────────────────────────────────── */}
          <section id="groups">
            <SectionHeading id="groups">👥 Group Messaging</SectionHeading>
            <p className="text-[#7b82a8] text-[13px] leading-relaxed mb-4">
              Group conversations use the <strong className="text-white">Sender Key</strong> protocol.
              The sender distributes their encrypted sender key to each member (via X3DH 1:1 channels), then uses it to broadcast messages efficiently.
              Groups support 3–1000 members (creator + 2–999 additional).
            </p>

            <SubHeading>Create a Group</SubHeading>
            <CodeBlock lang="bash">{`curl -X POST http://localhost:3000/api/groups \\
  -H "Authorization: Bearer eyJ..." \\
  -H "Content-Type: application/json" \\
  -d '{
    "members": [
      { "user_id": "bob",   "device_id": "..." },
      { "user_id": "carol", "device_id": "..." }
    ]
  }'`}</CodeBlock>
            <CodeBlock lang="json">{`{
  "conversation_id": "7a1b2c3d-...",
  "members": [
    { "user_id": "alice", "device_id": "..." },
    { "user_id": "bob",   "device_id": "..." },
    { "user_id": "carol", "device_id": "..." }
  ]
}`}</CodeBlock>

            <SubHeading>Distribute Sender Key</SubHeading>
            <p className="text-[#7b82a8] text-[13px] mb-2 leading-relaxed">
              After creating the group, distribute your Sender Key material to each member. Each recipient gets their own encrypted copy.
            </p>
            <CodeBlock lang="bash">{`curl -X POST http://localhost:3000/api/groups/{conversationId}/sender-key-distribution \\
  -H "Authorization: Bearer eyJ..." \\
  -H "Content-Type: application/json" \\
  -d '{
    "recipients": [
      { "user_id": "bob",   "device_id": "...", "encrypted_skdm": "<base64>" },
      { "user_id": "carol", "device_id": "...", "encrypted_skdm": "<base64>" }
    ]
  }'`}</CodeBlock>

            <SubHeading>Send a Group Message</SubHeading>
            <p className="text-[#7b82a8] text-[13px] mb-2">Once keys are distributed, send messages to all members with a single call. The server fans out to all member devices.</p>
            <CodeBlock lang="bash">{`curl -X POST http://localhost:3000/api/groups/{conversationId}/messages \\
  -H "Authorization: Bearer eyJ..." \\
  -H "Content-Type: application/json" \\
  -d '{
    "envelope": {
      "conversation_id": "7a1b2c3d-...",
      "ciphertext": "<base64 Sender-Key encrypted payload>",
      "protocol_header": {
        "type": "sender_key",
        "chain_id": 1,
        "iteration": 5
      }
    }
  }'`}</CodeBlock>

            <SubHeading>Manage Membership</SubHeading>
            <CodeBlock lang="bash">{`# Add a member
curl -X POST http://localhost:3000/api/groups/{conversationId}/members \\
  -H "Authorization: Bearer eyJ..." \\
  -H "Content-Type: application/json" \\
  -d '{ "user_id": "dave", "device_id": "..." }'

# Remove a member
curl -X DELETE http://localhost:3000/api/groups/{conversationId}/members/{userId} \\
  -H "Authorization: Bearer eyJ..."`}</CodeBlock>
            <Callout type="tip">When a new member joins, distribute your Sender Key to them individually (via a 1:1 X3DH channel) so they can decrypt future group messages.</Callout>
          </section>

          <Divider />

          {/* ── Attachments ───────────────────────────────────────────────────── */}
          <section id="attachments">
            <SectionHeading id="attachments">📎 File Attachments</SectionHeading>
            <p className="text-[#7b82a8] text-[13px] leading-relaxed mb-4">
              Files are uploaded separately and referenced in messages via their <Code>attachment_id</Code>.
              Files are stored with tenant isolation — a tenant can only access their own attachments.
            </p>

            <Callout type="info">Maximum file size: <strong>100 MB</strong>. Files are stored at <Code>ATTACHMENT_DIR/{`{tenant_id}`}/{`{attachment_id}`}</Code>.</Callout>

            <SubHeading>Upload a File</SubHeading>
            <CodeBlock lang="bash">{`curl -X POST http://localhost:3000/api/attachments \\
  -H "Authorization: Bearer eyJ..." \\
  -F "file=@/path/to/photo.jpg"`}</CodeBlock>
            <CodeBlock lang="json">{`{
  "attachment_id": "c3d4e5f6-...",
  "filename":      "photo.jpg",
  "content_type":  "image/jpeg",
  "size_bytes":    204800
}`}</CodeBlock>

            <SubHeading>Reference in a Message</SubHeading>
            <p className="text-[#7b82a8] text-[13px] mb-2">Include the <Code>attachment_id</Code> in your message envelope:</p>
            <CodeBlock lang="json">{`{
  "envelope": {
    "conversation_id": "...",
    "ciphertext": "<base64>",
    "protocol_header": { ... },
    "attachment_id": "c3d4e5f6-..."
  }
}`}</CodeBlock>

            <SubHeading>Download a File</SubHeading>
            <CodeBlock lang="bash">{`curl http://localhost:3000/api/attachments/{attachmentId} \\
  -H "Authorization: Bearer eyJ..." \\
  --output photo.jpg`}</CodeBlock>
            <p className="text-[#7b82a8] text-[13px] mt-2">Returns the file with correct <Code>Content-Type</Code> and <Code>Content-Disposition: attachment</Code> headers.</p>
          </section>

          <Divider />

          {/* ── WebSocket ─────────────────────────────────────────────────────── */}
          <section id="websocket">
            <SectionHeading id="websocket">⚡ WebSocket (Real-Time Events)</SectionHeading>
            <p className="text-[#7b82a8] text-[13px] leading-relaxed mb-4">
              Connect a persistent WebSocket to receive real-time events. The token is passed as a query parameter
              because the browser <Code>WebSocket</Code> API does not support custom headers.
            </p>

            <SubHeading>Connect</SubHeading>
            <CodeBlock lang="javascript">{`const ws = new WebSocket(
  'ws://localhost:3000/api/ws' +
  '?token=' + encodeURIComponent(jwt) +
  '&device_id=' + deviceId
);`}</CodeBlock>
            <Callout type="warning">Always include the <Code>device_id</Code> parameter. Without it, the server cannot route messages to the correct device session.</Callout>

            <SubHeading>Event Types</SubHeading>
            <div className="space-y-3">
              {[
                {
                  type: 'message',
                  desc: 'A new message was delivered to this device (1:1 or group).',
                  payload: `{
  "type": "message",
  "seq": 43,
  "conversation_id": "...",
  "sender_user_id": "alice",
  "sender_device_id": "...",
  "ciphertext": "<base64>",
  "protocol_header": { ... },
  "server_ts": 1700000000000,
  "attachment_id": null
}`,
                },
                {
                  type: 'low_otpk',
                  desc: 'Your OTPK pool is running low (< 10 remaining). Upload more OTPKs.',
                  payload: `{
  "type": "low_otpk",
  "device_id": "...",
  "count": 7
}`,
                },
                {
                  type: 'member_added',
                  desc: 'A new member joined a group conversation you are in.',
                  payload: `{
  "type": "member_added",
  "conversation_id": "...",
  "user_id": "dave",
  "devices": ["..."]
}`,
                },
                {
                  type: 'member_removed',
                  desc: 'A member was removed from a group conversation you are in.',
                  payload: `{
  "type": "member_removed",
  "conversation_id": "...",
  "user_id": "dave"
}`,
                },
                {
                  type: 'sender_key_distribution',
                  desc: 'Received a Sender Key distribution message for a group.',
                  payload: `{
  "type": "sender_key_distribution",
  "conversation_id": "...",
  "sender_user_id": "alice",
  "encrypted_skdm": "<base64>"
}`,
                },
              ].map(e => (
                <div key={e.type} className="bg-[#1a1d27] border border-[#2e3250] rounded-xl overflow-hidden">
                  <div className="flex items-center gap-3 px-4 py-2.5 border-b border-[#2e3250]">
                    <Badge color="purple">{e.type}</Badge>
                    <span className="text-[13px] text-[#c9cce0]">{e.desc}</span>
                  </div>
                  <CodeBlock lang="json">{e.payload}</CodeBlock>
                </div>
              ))}
            </div>

            <SubHeading>Offline Queue</SubHeading>
            <p className="text-[#7b82a8] text-[13px] leading-relaxed">
              Messages sent while your device is offline are queued in the database. They are automatically drained and delivered when you reconnect your WebSocket.
              No polling required — simply reconnecting triggers delivery of any missed events.
            </p>
          </section>

          <Divider />

          {/* ── Errors ────────────────────────────────────────────────────────── */}
          <section id="errors">
            <SectionHeading id="errors">🔴 Error Reference</SectionHeading>
            <p className="text-[#7b82a8] text-[13px] leading-relaxed mb-4">
              All errors return a consistent JSON envelope with an <Code>error_code</Code>, human-readable <Code>message</Code>, and a unique <Code>request_id</Code> for support correlation.
            </p>
            <CodeBlock lang="json">{`{
  "error_code": "bad_request",
  "message":    "A SignedPreKey signature could not be verified.",
  "request_id": "ebd6456e-..."
}`}</CodeBlock>
            <div className="overflow-x-auto mt-4">
              <table className="w-full text-left text-[13px] border-collapse">
                <thead>
                  <tr className="border-b border-[#2e3250] text-[#7b82a8] text-[12px]">
                    <th className="py-2 px-3 font-medium">HTTP Status</th>
                    <th className="py-2 px-3 font-medium">error_code</th>
                    <th className="py-2 px-3 font-medium">Cause</th>
                  </tr>
                </thead>
                <tbody className="text-[#c9cce0]">
                  {[
                    ['400', 'bad_request', 'Malformed request body or invalid parameters.'],
                    ['401', 'unauthorized', 'Missing, expired, or invalid Bearer token.'],
                    ['403', 'forbidden', 'Authenticated but not authorized for this resource.'],
                    ['404', 'not_found', 'The requested resource does not exist.'],
                    ['409', 'device_limit_reached', 'User already has 5 registered devices.'],
                    ['413', 'payload_too_large', 'Uploaded file exceeds the 100 MB limit.'],
                    ['422', 'invalid_signed_prekey_signature', 'Ed25519 signature on SignedPreKey is invalid.'],
                    ['500', 'internal_error', 'Unexpected server error.'],
                    ['503', 'storage_unavailable', 'Database or storage layer is temporarily unavailable.'],
                  ].map(([status, code, cause]) => (
                    <tr key={code} className="border-b border-[#2e3250] hover:bg-[#1e2235] transition-colors">
                      <td className="py-2 px-3"><Badge color={status === '200' || status === '201' ? 'green' : status === '400' || status === '422' ? 'yellow' : 'red'}>{status}</Badge></td>
                      <td className="py-2 px-3 font-mono text-[12px] text-[#a5aff9]">{code}</td>
                      <td className="py-2 px-3 text-[#7b82a8]">{cause}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>

          <Divider />

          {/* ── Admin API ─────────────────────────────────────────────────────── */}
          <section id="admin">
            <SectionHeading id="admin">🛠️ Admin API</SectionHeading>
            <p className="text-[#7b82a8] text-[13px] leading-relaxed mb-4">
              Admin endpoints require the <Code>ADMIN_TOKEN</Code> set in your <Code>.env</Code> file passed as a Bearer token. These are not exposed to end-users.
            </p>
            <div className="overflow-x-auto">
              <table className="w-full text-left text-[13px] border-collapse">
                <thead>
                  <tr className="border-b border-[#2e3250] text-[#7b82a8] text-[12px]">
                    <th className="py-2 px-3 font-medium">Method</th>
                    <th className="py-2 px-3 font-medium">Path</th>
                    <th className="py-2 px-3 font-medium">Auth</th>
                    <th className="py-2 px-3 font-medium">Description</th>
                  </tr>
                </thead>
                <tbody>
                  <RouteRow method="GET"    path="/admin/tenants"                          auth="ADMIN_TOKEN" desc="List all tenants" />
                  <RouteRow method="POST"   path="/admin/tenants"                          auth="ADMIN_TOKEN" desc="Create a new tenant" />
                  <RouteRow method="DELETE" path="/admin/tenants/:id"                      auth="ADMIN_TOKEN" desc="Deactivate a tenant" />
                  <RouteRow method="PUT"    path="/admin/tenants/:id/oidc"                 auth="ADMIN_TOKEN" desc="Update tenant OIDC issuer" />
                  <RouteRow method="GET"    path="/admin/tenants/:id/usage"                auth="ADMIN_TOKEN" desc="Get usage metrics for a tenant" />
                  <RouteRow method="GET"    path="/admin/registrations"                    auth="ADMIN_TOKEN" desc="List all registration requests" />
                  <RouteRow method="POST"   path="/admin/registrations/:id/approve"        auth="ADMIN_TOKEN" desc="Approve a registration (provisions tenant)" />
                  <RouteRow method="POST"   path="/admin/registrations/:id/reject"         auth="ADMIN_TOKEN" desc="Reject a registration with optional reason" />
                </tbody>
              </table>
            </div>

            <SubHeading>All Routes Summary</SubHeading>
            <div className="overflow-x-auto">
              <table className="w-full text-left text-[13px] border-collapse">
                <thead>
                  <tr className="border-b border-[#2e3250] text-[#7b82a8] text-[12px]">
                    <th className="py-2 px-3 font-medium">Method</th>
                    <th className="py-2 px-3 font-medium">Path</th>
                    <th className="py-2 px-3 font-medium">Auth</th>
                    <th className="py-2 px-3 font-medium">Description</th>
                  </tr>
                </thead>
                <tbody>
                  <RouteRow method="GET"  path="/health"          auth="None" desc="Health check" />
                  <RouteRow method="GET"  path="/metrics"         auth="None" desc="Prometheus metrics" />
                  <RouteRow method="POST" path="/auth/refresh"    auth="None" desc="Refresh access token" />
                  <RouteRow method="POST" path="/registrations"   auth="None" desc="Submit self-registration" />
                  <RouteRow method="GET"  path="/registrations/:id" auth="Registration Token" desc="Get registration status" />
                  <RouteRow method="POST" path="/users/:id/devices" auth="JWT" desc="Register a device + key bundle" />
                  <RouteRow method="GET"  path="/users/:id/key-bundle" auth="JWT" desc="Fetch a user's key bundle (consumes OTPK)" />
                  <RouteRow method="PUT"  path="/users/:id/devices/:deviceId/one-time-prekeys" auth="JWT" desc="Replenish OTPKs" />
                  <RouteRow method="PUT"  path="/users/:id/devices/:deviceId/signed-prekey" auth="JWT" desc="Rotate signed pre-key" />
                  <RouteRow method="POST" path="/conversations" auth="JWT" desc="Create conversation / send first message (X3DH)" />
                  <RouteRow method="POST" path="/conversations/:id/messages" auth="JWT" desc="Send message (Double Ratchet)" />
                  <RouteRow method="GET"  path="/conversations/:id/messages" auth="JWT" desc="Fetch message history" />
                  <RouteRow method="POST" path="/groups" auth="JWT" desc="Create group conversation" />
                  <RouteRow method="POST" path="/groups/:id/messages" auth="JWT" desc="Send group message (Sender Key)" />
                  <RouteRow method="POST" path="/groups/:id/members" auth="JWT" desc="Add group member" />
                  <RouteRow method="DELETE" path="/groups/:id/members/:userId" auth="JWT" desc="Remove group member" />
                  <RouteRow method="POST" path="/groups/:id/sender-key-distribution" auth="JWT" desc="Distribute Sender Key material" />
                  <RouteRow method="POST" path="/attachments" auth="JWT" desc="Upload file attachment" />
                  <RouteRow method="GET"  path="/attachments/:id" auth="JWT" desc="Download attachment by ID" />
                  <RouteRow method="GET"  path="/ws" auth="JWT (query param)" desc="WebSocket real-time connection" />
                </tbody>
              </table>
            </div>
          </section>

          <Divider />

          {/* ── Dev Client ────────────────────────────────────────────────────── */}
          <section id="dev-client">
            <SectionHeading id="dev-client">🖥️ Dev Client Guide</SectionHeading>
            <p className="text-[#7b82a8] text-[13px] leading-relaxed mb-4">
              The built-in Dev Client at <Code>/test</Code> lets you test the full platform without writing any code.
              It is available in both dev and production mode.
            </p>

            <SubHeading>Step 1 — Register a user (Dev Mode)</SubHeading>
            <ol className="list-decimal list-inside text-[#c9cce0] space-y-2 text-[13px]">
              <li>Open <Code>http://localhost:3000/test</Code> and go to the <strong>Users</strong> tab.</li>
              <li>Under <em>Get Token (Mock OIDC)</em>, type a username (e.g. <Code>alice</Code>) and click <strong>Issue Token</strong>.</li>
              <li>The JWT auto-fills. Click <strong>Register &amp; Create User</strong> — this calls <Code>POST /users/alice/devices</Code> and uploads a generated key bundle.</li>
              <li>Repeat for a second user (e.g. <Code>bob</Code>).</li>
            </ol>

            <SubHeading>Step 2 — Start Chatting</SubHeading>
            <ol className="list-decimal list-inside text-[#c9cce0] space-y-2 text-[13px]">
              <li>On Alice's card, click <strong>Use in Chat</strong>.</li>
              <li>Switch to the <strong>Chat</strong> tab — the WebSocket connects automatically.</li>
              <li>Fill in Bob's <strong>Recipient User ID</strong> and <strong>Device ID</strong> (copy from his card).</li>
              <li>Click <strong>Start Conversation</strong>, then type and send a message.</li>
              <li>To test real-time delivery, open a second browser tab at the same URL, use Bob's identity, and open the conversation. Messages from Alice appear instantly.</li>
            </ol>

            <SubHeading>Step 3 — Send an Attachment</SubHeading>
            <ol className="list-decimal list-inside text-[#c9cce0] space-y-2 text-[13px]">
              <li>In any open conversation, click the <strong>📎</strong> button.</li>
              <li>Select a file (max 100 MB). The client uploads it and embeds the <Code>attachment_id</Code> in the message.</li>
              <li>The recipient sees an inline preview for images, a player for audio/video, or a download link for other files.</li>
            </ol>

            <Callout type="tip">
              The Dev Client persists your identity (token, user ID, device ID) to <Code>localStorage</Code> so you do not need to re-register across page refreshes.
            </Callout>
          </section>

          <div className="pb-12" />
        </main>
      </div>
    </div>
  );
}
