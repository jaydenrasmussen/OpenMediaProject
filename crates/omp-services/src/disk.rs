/// Free space lookup, injected so tests and platforms can differ.
pub trait DiskSpace: Send + Sync {
    /// Free bytes on the volume holding `path`; `None` when unknown.
    fn free_bytes(&self, path: &str) -> Option<u64>;
}

/// Reports nothing, so the free-space check passes. The default until a real
/// disk probe is wired in.
pub struct NoDiskCheck;

impl DiskSpace for NoDiskCheck {
    fn free_bytes(&self, _path: &str) -> Option<u64> {
        None
    }
}
