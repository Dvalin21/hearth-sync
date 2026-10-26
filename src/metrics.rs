use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct Metrics {
    pub requests_total: AtomicU64,
    pub photos_uploaded: AtomicU64,
    pub auth_failures: AtomicU64,
}

impl Metrics {
    pub fn render(&self) -> String {
        format!(
            "# HELP hearth_sync_requests_total Total HTTP requests\n\
             # TYPE hearth_sync_requests_total counter\n\
             hearth_sync_requests_total {}\n\
             # HELP hearth_sync_photos_uploaded Total photos uploaded\n\
             # TYPE hearth_sync_photos_uploaded counter\n\
             hearth_sync_photos_uploaded {}\n\
             # HELP hearth_sync_auth_failures Total auth failures\n\
             # TYPE hearth_sync_auth_failures counter\n\
             hearth_sync_auth_failures {}\n",
            self.requests_total.load(Ordering::Relaxed),
            self.photos_uploaded.load(Ordering::Relaxed),
            self.auth_failures.load(Ordering::Relaxed),
        )
    }
}
