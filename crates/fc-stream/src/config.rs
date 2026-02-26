/// Configuration for the PostgreSQL projection stream processor.
pub struct StreamProcessorConfig {
    /// Enable the event projection loop
    pub events_enabled: bool,
    /// Max rows per event projection poll cycle
    pub events_batch_size: u32,
    /// Enable the dispatch job projection loop
    pub dispatch_jobs_enabled: bool,
    /// Max rows per dispatch job projection poll cycle
    pub dispatch_jobs_batch_size: u32,
}

impl Default for StreamProcessorConfig {
    fn default() -> Self {
        Self {
            events_enabled: true,
            events_batch_size: 100,
            dispatch_jobs_enabled: true,
            dispatch_jobs_batch_size: 100,
        }
    }
}
