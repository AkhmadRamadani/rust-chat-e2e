# Requirements Document

## Introduction

A Rust-based multi-tenant SaaS chat API providing encrypted messaging for 1:1 and group conversations, deployed as a **multi-tenant SaaS platform**. Multiple independent tenant applications share a single deployment, with complete data isolation between tenants enforced at the application layer via a shared PostgreSQL schema. Each tenant brings their own OAuth 2.0 / OIDC identity provider; the platform resolves the tenant from the JWT `iss` claim and validates tokens against the tenant's configured JWKS endpoint. The system uses HTTP/1.1 with WebSocket upgrade for real-time message delivery, implements the Signal Protocol (X3DH key agreement + Double Ratchet messaging) for end-to-end encryption, and manages group session keys via Sender Keys. A Key Distribution Server stores and serves public key bundles required for session establishment. REST endpoints over HTTP/1.1 handle registration, key management, and message history retrieval, while WebSocket connections carry real-time encrypted message delivery.

## Glossary

- **ChatAPI**: The Rust server application that exposes all HTTP/1.1 REST endpoints and WebSocket sessions.
- **Client**: Any application connecting to the ChatAPI on behalf of an authenticated user.
- **Tenant**: An independent application or organization using the ChatAPI as a SaaS platform, identified by a TenantID. Each tenant's data is fully isolated from all other tenants.
- **TenantID**: A server-assigned UUID identifying a Tenant.
- **Admin API**: A separate management API protected by a platform-level admin token, used to provision and configure tenants.
- **OIDC Issuer**: The unique OIDC `iss` claim URL that identifies a tenant's identity provider and is used to resolve the JWKS endpoint.
- **KDS**: Key Distribution Server — the subsystem of the ChatAPI that stores and serves public key bundles.
- **KeyBundle**: A user's collection of public keys including the identity key, signed pre-key, and one-time pre-keys used for X3DH key agreement.
- **X3DH**: Extended Triple Diffie-Hellman — the key agreement protocol used to establish a shared secret before the first message in a conversation.
- **DoubleRatchet**: The Double Ratchet Algorithm used to derive per-message encryption keys after X3DH session establishment.
- **SenderKey**: A per-group symmetric key distributed to group members and used to encrypt/decrypt group messages under the Sender Keys scheme.
- **IdentityKey**: A user's long-term Curve25519 public/private key pair used in X3DH.
- **SignedPreKey**: A medium-term Curve25519 key pair signed by the IdentityKey, published to the KDS, and rotated periodically.
- **OneTimePreKey**: A single-use Curve25519 key pair uploaded to the KDS and consumed during X3DH session initiation.
- **Conversation**: A persistent 1:1 or group messaging thread identified by a unique ConversationID, scoped to a Tenant.
- **ConversationID**: A server-assigned UUID identifying a Conversation within a Tenant.
- **MessageEnvelope**: A server-side record containing the ciphertext, sender metadata, and delivery status of a single message.
- **WebSocketSession**: A persistent bidirectional WebSocket connection between a Client and the ChatAPI used for real-time message delivery.
- **OIDC**: OpenID Connect — the identity layer on top of OAuth 2.0 used to authenticate users.
- **AccessToken**: A short-lived OAuth 2.0 bearer token presented by the Client on every request.
- **RefreshToken**: A longer-lived token used to obtain a new AccessToken without re-authenticating.
- **DeviceID**: A server-assigned identifier for a specific device registration of a user account, unique within a Tenant.
- **UserID**: An identifier for a user account derived from the OIDC subject claim, unique within a Tenant.

---

## Requirements

### Requirement 0: Tenant Management

**User Story:** As a platform operator, I want to provision and manage tenants so that multiple independent applications can use the ChatAPI in complete data isolation from each other.

#### Acceptance Criteria

1. THE ChatAPI SHALL expose an Admin API authenticated via a platform-level bearer token (distinct from tenant user tokens) with endpoints for tenant lifecycle management.
2. WHEN an operator submits `POST /admin/tenants` with a tenant `name` and `oidc_issuer` URL, THE ChatAPI SHALL provision a new tenant, assign a unique TenantID, store the OIDC issuer, and return HTTP 201 with the TenantID.
3. THE ChatAPI SHALL store each tenant's OIDC issuer URL and use it to resolve the JWKS endpoint for validating user tokens belonging to that tenant.
4. WHEN an operator submits `DELETE /admin/tenants/{tenantID}`, THE ChatAPI SHALL mark the tenant as inactive and reject all future requests bearing that tenant's tokens with HTTP 403 and error code `tenant_inactive`.
5. WHEN an operator submits `GET /admin/tenants/{tenantID}/usage`, THE ChatAPI SHALL return current usage metrics: active user count, device count, message count (last 30 days), and active WebSocket session count.
6. THE ChatAPI SHALL enforce that all Admin API endpoints are inaccessible without a valid platform admin token; requests without a valid admin token SHALL receive HTTP 401.
7. WHEN an operator submits `PUT /admin/tenants/{tenantID}/oidc`, THE ChatAPI SHALL update the tenant's OIDC issuer URL and invalidate the JWKS cache for that tenant.

---

### Requirement 1: Transport Layer

**User Story:** As a client developer, I want all API communication to run over HTTP/1.1 with WebSocket upgrade so that I can use standard browser and server tooling without special protocol requirements.

#### Acceptance Criteria

1. THE ChatAPI SHALL accept incoming HTTP/1.1 connections over TCP and serve all REST endpoints.
2. THE ChatAPI SHALL support WebSocket upgrade on the `GET /ws` endpoint; connections presenting a valid JWT token in the `token` query parameter SHALL be upgraded to a WebSocket session.
3. WHEN a WebSocket session is established, THE ChatAPI SHALL authenticate the connection using the `token` query parameter; invalid or expired tokens SHALL result in HTTP 401 before the upgrade.
4. THE ChatAPI SHALL maintain WebSocket sessions until either the Client closes the connection or an idle timeout of 120 seconds without any message activity expires.
5. WHEN a WebSocket session is established, THE ChatAPI SHALL send a `ping` frame every 30 seconds on idle sessions; WHEN no `pong` is received within 10 seconds, THE ChatAPI SHALL close the session.

---

### Requirement 2: Authentication and Authorization

**User Story:** As a platform operator, I want all API access to be gated by OAuth 2.0/OIDC so that only authenticated users with valid tokens can send or receive messages, and tenant data is never exposed across tenant boundaries.

#### Acceptance Criteria

1. THE ChatAPI SHALL validate every incoming HTTP request against an AccessToken presented in the HTTP Authorization header using the Bearer scheme. Before validating the token signature, THE ChatAPI SHALL extract the `iss` claim and resolve the corresponding TenantID from the tenant registry; if the issuer is unknown, THE ChatAPI SHALL return HTTP 401 with error code `unknown_tenant`.
2. WHEN an AccessToken is absent or fails OIDC signature verification, THE ChatAPI SHALL return HTTP 401 with a `WWW-Authenticate: Bearer` header.
3. WHEN an AccessToken has expired, THE ChatAPI SHALL return HTTP 401 with an `error=invalid_token` parameter in the WWW-Authenticate header.
4. THE ChatAPI SHALL support AccessToken refresh via a dedicated `/auth/refresh` endpoint that accepts a RefreshToken and returns a new AccessToken with a lifetime of 3600 seconds.
5. WHEN a RefreshToken is invalid, revoked, or expired, THE ChatAPI SHALL return HTTP 401 and invalidate any active sessions associated with that RefreshToken.
6. THE ChatAPI SHALL extract the TenantID from the resolved `iss` claim and the UserID from the `sub` claim of the validated OIDC token, and associate both with all server-side records created during the authenticated session.
7. THE ChatAPI SHALL enforce that a Client may only access Conversations, KeyBundles, and MessageEnvelopes associated with the UserID derived from the presented AccessToken, within the same Tenant.
8. WHEN a Client attempts to access a resource owned by a different UserID within the same Tenant, THE ChatAPI SHALL return HTTP 403.
9. THE ChatAPI SHALL enforce strict tenant isolation: a token issued for Tenant A SHALL never grant access to any resource owned by Tenant B; cross-tenant access attempts SHALL return HTTP 403 with error code `forbidden`.

---

### Requirement 3: Device Registration

**User Story:** As a user, I want to register my device and upload my key material so that other users within the same tenant can discover my public keys and initiate encrypted sessions with me.

#### Acceptance Criteria

1. WHEN a Client submits a `POST /users/{userID}/devices` request with a valid AccessToken and a well-formed KeyBundle, THE ChatAPI SHALL register the device scoped to the resolved TenantID, assign a unique DeviceID, and return HTTP 201 with the assigned DeviceID.
2. THE ChatAPI SHALL verify that the SignedPreKey in the submitted KeyBundle carries a valid Ed25519 signature over the pre-key's public value using the provided IdentityKey before storing the KeyBundle.
3. IF the SignedPreKey signature verification fails, THEN THE ChatAPI SHALL return HTTP 422 with an error code of `invalid_signed_prekey_signature`.
4. THE ChatAPI SHALL accept a minimum of 1 and a maximum of 100 OneTimePreKeys per registration or upload request.
5. IF a registration request contains zero OneTimePreKeys, THEN THE ChatAPI SHALL store the KeyBundle without OneTimePreKeys and set the device's one-time pre-key count to 0.
6. THE ChatAPI SHALL allow a single UserID (within a Tenant) to register up to 5 concurrent DeviceIDs.
7. IF a UserID already has 5 registered DeviceIDs and a new registration is attempted, THEN THE ChatAPI SHALL return HTTP 409 with error code `device_limit_reached`.

---

### Requirement 4: Key Distribution Server (KDS)

**User Story:** As a client initiating a new encrypted session, I want to fetch the recipient's public key bundle from the KDS so that I can perform X3DH key agreement without prior out-of-band exchange.

#### Acceptance Criteria

1. WHEN a Client submits a `GET /users/{userID}/key-bundle` request with a valid AccessToken, THE KDS SHALL return the IdentityKey, the current SignedPreKey, the SignedPreKey's signature, and one OneTimePreKey (if available) for one registered device of the specified UserID within the same Tenant.
2. WHEN a OneTimePreKey is returned in a key bundle fetch, THE KDS SHALL atomically mark that OneTimePreKey as consumed and remove it from the available pool so it is never returned again.
3. WHILE a device's OneTimePreKey pool is empty, THE KDS SHALL return a key bundle containing the IdentityKey and SignedPreKey only, without a OneTimePreKey field, and SHALL include a `x-otpk-warning: depleted` response header.
4. THE KDS SHALL notify the owning Client via an active WebSocketSession when the device's OneTimePreKey count drops below 10, delivering a `low_otpk` event with the current count.
5. WHEN a Client submits a `PUT /users/{userID}/devices/{deviceID}/one-time-prekeys` request with a valid AccessToken and a list of between 1 and 100 OneTimePreKeys, THE KDS SHALL append the new keys to the device's pool and return HTTP 200 with the updated total count.
6. THE KDS SHALL rotate SignedPreKeys: WHEN a Client submits a `PUT /users/{userID}/devices/{deviceID}/signed-prekey` request with a valid new SignedPreKey and signature, THE KDS SHALL verify the signature against the stored IdentityKey and, if valid, replace the current SignedPreKey and return HTTP 200.
7. IF the new SignedPreKey signature verification fails during rotation, THEN THE KDS SHALL return HTTP 422 with error code `invalid_signed_prekey_signature` and retain the existing SignedPreKey unchanged.

---

### Requirement 5: 1:1 Session Establishment (X3DH)

**User Story:** As a sender, I want to establish a forward-secret encrypted session with a recipient using X3DH so that both parties share a session key without having been online simultaneously.

#### Acceptance Criteria

1. THE ChatAPI SHALL treat X3DH session establishment as a client-side operation; the server SHALL only store and serve the resulting initial MessageEnvelope along with the sender's ephemeral public key and pre-key identifiers used during the X3DH handshake.
2. WHEN a Client submits a `POST /conversations` request containing an initial MessageEnvelope with X3DH header fields (`sender_identity_key`, `ephemeral_key`, `used_signed_prekey_id`, and optionally `used_otpk_id`), THE ChatAPI SHALL create a new Conversation scoped to the resolved TenantID, assign a ConversationID, store the MessageEnvelope, and return HTTP 201 with the ConversationID.
3. THE ChatAPI SHALL store X3DH header fields alongside the initial MessageEnvelope so the recipient can perform the X3DH derivation upon first message retrieval.
4. IF a 1:1 Conversation already exists between the same two participants within the same Tenant, THEN THE ChatAPI SHALL return the existing ConversationID in HTTP 200 instead of creating a duplicate.
5. THE ChatAPI SHALL NOT decrypt, inspect, or modify message ciphertext at any point; all encryption and decryption SHALL be performed exclusively by Clients.

---

### Requirement 6: 1:1 Messaging (Double Ratchet)

**User Story:** As a user in a 1:1 conversation, I want every message encrypted with a unique per-message key derived via the Double Ratchet so that compromise of one message key does not expose past or future messages.

#### Acceptance Criteria

1. THE ChatAPI SHALL accept `POST /conversations/{conversationID}/messages` requests containing an opaque ciphertext blob and a Double Ratchet protocol header.
2. THE ChatAPI SHALL store each MessageEnvelope with a server-assigned monotonically increasing sequence number scoped to the ConversationID.
3. WHEN a MessageEnvelope is stored, THE ChatAPI SHALL deliver it to all active WebSocketSessions belonging to the recipient DeviceID within 500 milliseconds.
4. WHILE a recipient has no active WebSocketSession, THE ChatAPI SHALL queue up to 10,000 undelivered MessageEnvelopes per ConversationID per DeviceID and deliver them in sequence-number order when a WebSocketSession is next established.
5. IF the undelivered queue for a DeviceID exceeds 10,000 MessageEnvelopes, THEN THE ChatAPI SHALL discard the oldest MessageEnvelopes beyond that limit and increment a dropped count.
6. THE ChatAPI SHALL associate each stored MessageEnvelope with a server-side timestamp (Unix epoch milliseconds) at the time of receipt.

---

### Requirement 7: Group Conversations and Sender Keys

**User Story:** As a user, I want to participate in group conversations where each sender's messages are encrypted with their own Sender Key so that the server never has access to plaintext and key compromise is isolated per sender.

#### Acceptance Criteria

1. WHEN a Client submits a `POST /groups` request with a valid AccessToken and a list of at least 2 and at most 999 additional member UserIDs, THE ChatAPI SHALL create a group Conversation scoped to the resolved TenantID, assign a ConversationID, and return HTTP 201 with the ConversationID and the final member list.
2. THE ChatAPI SHALL store group membership as a set of (UserID, DeviceID) pairs and update it when members are added or removed.
3. THE ChatAPI SHALL NOT generate, store, or have access to SenderKey material; SenderKeys SHALL be generated exclusively by Clients and distributed to group members as encrypted SenderKey distribution messages.
4. WHEN a Client posts a SenderKey distribution message to `POST /groups/{conversationID}/sender-key-distribution`, THE ChatAPI SHALL store one encrypted copy of the distribution message per recipient (UserID, DeviceID) pair and deliver each copy only to the corresponding recipient.
5. WHEN a new member is added to a group via `POST /groups/{conversationID}/members`, THE ChatAPI SHALL notify all existing group members via their active WebSocketSessions with a `member_added` event containing the new member's UserID and DeviceID list within 1 second.
6. WHEN a member is removed from a group via `DELETE /groups/{conversationID}/members/{userID}`, THE ChatAPI SHALL notify all remaining members via their active WebSocketSessions with a `member_removed` event within 1 second.
7. THE ChatAPI SHALL enforce that only existing group members may submit messages or SenderKey distribution messages to a group Conversation; non-member submissions SHALL receive HTTP 403.

---

### Requirement 8: Group Messaging

**User Story:** As a group member, I want encrypted group messages fanned out to all active members so that real-time group communication is reliable and timely.

#### Acceptance Criteria

1. WHEN a Client submits a `POST /groups/{conversationID}/messages` request with an encrypted ciphertext and Sender Key ratchet header, THE ChatAPI SHALL store the MessageEnvelope and fan it out to all current group member (UserID, DeviceID) pairs.
2. THE ChatAPI SHALL assign a server-side monotonically increasing sequence number scoped to the group ConversationID to each MessageEnvelope.
3. WHEN a group MessageEnvelope is stored, THE ChatAPI SHALL deliver it to all active WebSocketSessions of group members within 500 milliseconds.
4. WHILE a group member has no active WebSocketSession, THE ChatAPI SHALL queue undelivered MessageEnvelopes per (ConversationID, DeviceID) pair subject to the same 10,000-envelope limit and drop policy specified in Requirement 6.
5. THE ChatAPI SHALL reject message submissions from a DeviceID that is not registered to a current group member UserID with HTTP 403.

---

### Requirement 9: Message History

**User Story:** As a user, I want to retrieve past messages in a conversation so that I can read conversation history after reconnecting or switching devices.

#### Acceptance Criteria

1. WHEN a Client submits a `GET /conversations/{conversationID}/messages` request with a valid AccessToken, THE ChatAPI SHALL return a paginated list of MessageEnvelopes for that Conversation ordered by ascending sequence number.
2. THE ChatAPI SHALL support cursor-based pagination via `before_seq` and `limit` query parameters, where `limit` defaults to 50 and SHALL NOT exceed 200 per request.
3. THE ChatAPI SHALL include each MessageEnvelope's sequence number, server receipt timestamp, sender UserID, sender DeviceID, ciphertext blob, and protocol header fields in the response.
4. THE ChatAPI SHALL enforce that only participants in a Conversation may retrieve its message history; non-participant requests SHALL receive HTTP 403.
5. THE ChatAPI SHALL retain MessageEnvelopes for a minimum of 30 days from the server receipt timestamp, after which they MAY be purged.

---

### Requirement 10: WebSocket Real-Time Delivery

**User Story:** As a client, I want to receive new messages and system events over a persistent WebSocket connection so that I do not need to poll for updates.

#### Acceptance Criteria

1. WHEN a Client connects to `GET /ws?token=<jwt>` with a valid JWT, THE ChatAPI SHALL upgrade the HTTP connection to a WebSocket session and associate it with the authenticated TenantID, UserID, and DeviceID. The DeviceID is derived from the JWT `device_id` claim if present, otherwise defaults to the device registered for that user.
2. THE ChatAPI SHALL multiplex all real-time events for a given (TenantID, UserID, DeviceID) tuple over a single WebSocket session; each event SHALL be delivered as a JSON text frame.
3. WHEN the ChatAPI delivers a MessageEnvelope over a WebSocketSession, THE ChatAPI SHALL include the ConversationID, sequence number, sender UserID, sender DeviceID, ciphertext blob, and protocol header in the JSON payload.
4. THE ChatAPI SHALL support client acknowledgement of received MessageEnvelopes; WHEN a Client sends an `ack` JSON message containing a sequence number, THE ChatAPI SHALL mark the corresponding MessageEnvelope as delivered for that DeviceID.
5. WHEN a WebSocketSession is closed by the Client or times out, THE ChatAPI SHALL transition all unacknowledged MessageEnvelopes associated with that DeviceID back to the undelivered queue.
6. THE ChatAPI SHALL send a `{"type":"ping"}` JSON message every 30 seconds on idle sessions; WHEN no `{"type":"pong"}` is received within 10 seconds, THE ChatAPI SHALL close the session.

---

### Requirement 11: Error Handling and Observability

**User Story:** As a platform operator, I want the API to return structured errors and expose metrics so that I can monitor health and diagnose issues across all tenants in production.

#### Acceptance Criteria

1. WHEN the ChatAPI encounters an error processing any request, THE ChatAPI SHALL return a JSON response body containing `error_code` (a machine-readable string), `message` (a human-readable description), and `request_id` (a UUID traced through all internal components).
2. THE ChatAPI SHALL expose a `/health` endpoint that returns HTTP 200 with a JSON body indicating the status of the HTTP listener, the KDS storage backend, and the message queue subsystem.
3. THE ChatAPI SHALL emit structured log entries in JSON format for every request, including the TenantID, UserID, DeviceID (where applicable), endpoint path, HTTP status code, and latency in milliseconds.
4. THE ChatAPI SHALL expose Prometheus-compatible metrics at `/metrics`, including counters for messages received, messages delivered, WebSocket sessions active, OneTimePreKey pool levels, and authentication failures. All per-tenant metrics SHALL include a `tenant_id` label.
5. IF an internal storage operation fails, THEN THE ChatAPI SHALL return HTTP 503 with error code `storage_unavailable` and SHALL NOT return partial or inconsistent data to the Client.

---

## Correctness Properties

### Property 1: Token Validation and UserID Extraction
*For any* HTTP request, if the presented AccessToken is absent, has an invalid OIDC signature, or is expired, the server SHALL respond with HTTP 401. For any valid token, the server SHALL extract the TenantID from the `iss` claim and the UserID exclusively from the `sub` claim, and SHALL use those values to associate all records created during the session.

**Validates: Requirements 2.1, 2.2, 2.3, 2.6**

### Property 2: Authorization Isolation
*For any* authenticated request where the token belongs to UserID U within Tenant T, the response SHALL never include Conversations, MessageEnvelopes, or KeyBundle data belonging to any UserID V ≠ U within the same tenant, and any attempt to access such data SHALL return HTTP 403.

**Validates: Requirements 2.7, 2.8, 9.4**

### Properties 3–14
All existing correctness properties 3 through 14 remain in force as originally specified, with references to "WebTransport" replaced by "WebSocket" and "HTTP/3" replaced by "HTTP/1.1".

### Property 15: Cross-Tenant Isolation
*For any* two distinct TenantIDs A and B, no operation performed with a token issued for Tenant A SHALL read, write, or enumerate any row with `tenant_id = B`. The system state for Tenant B SHALL be identical before and after any request made with Tenant A's credentials.

**Validates: Requirements 0.4, 0.6, 2.9**
