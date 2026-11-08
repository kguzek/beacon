//! Response types live in the shared `beacon-api` crate so clients can
//! deserialize exactly what the publisher serializes.
pub use beacon_api::{
    Current, CurrentService, History, HistoryKind, HistoryService, Probe, Probes, Segment,
    Segments, Status,
};
