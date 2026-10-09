//! Clients for services behind Oblivious HTTP (RFC 9458).

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub use ohttp_client::{KeyConfig, OhttpRequest, Url, parse_key_config};

pub mod mailbox_api;
