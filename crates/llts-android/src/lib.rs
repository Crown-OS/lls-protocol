//! CrownConnect for Android.
//!
//! Kotlin owns the platform: permissions, BLE scanning and advertising, NSD, MediaProjection and
//! the views. This owns the protocol: one runtime thread drives the llts [`Driver`] and the
//! [`Node`] orchestrator, and the hardware codecs feed it directly. What crosses the JNI boundary
//! is small and cold — commands in, JSON events out, beacons, and `Surface`s — never media, which
//! goes from `MediaCodec` buffers to the socket without the JVM seeing a byte of it.
//!
//! [`Driver`]: llts_transport::session::Driver
//! [`Node`]: llts_node::Node

pub mod bridge;
mod command;
mod config;
mod error;
mod event_json;
mod input_json;
mod media;
mod runtime;
mod state_json;
mod store;
mod util;
