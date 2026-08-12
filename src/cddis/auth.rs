//! Earthdata Login (URS) bearer-token auth: attaches
//! `Authorization: Bearer <token>` to CDDIS requests. No cookie jar or
//! redirect-following login flow is needed — confirmed against the live
//! archive, CDDIS accepts a URS token directly on the file request itself.
//!
//! Phase 2 of the project plan. Not yet implemented.
