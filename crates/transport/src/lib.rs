//! Transport crate — now a thin shim kept for compatibility.
//!
//! The QUIC/HTTP-3/WebTransport implementation has been replaced with
//! plain WebSocket via axum's built-in `WebSocketUpgrade` extractor.
//! Real-time session handling lives in `crates/realtime`.
//!
//! This crate is intentionally empty; the `api` crate wires the `/ws`
//! route directly using `axum::extract::ws`.
