//! Routes `tracing` (ours and iroh's) to logcat on Android, stderr elsewhere. `RUST_LOG`-style
//! filtering on desktop; Android keeps iroh at warn so logcat stays readable.

use std::sync::Once;

pub fn init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        use tracing_subscriber::prelude::*;
        #[cfg(target_os = "android")]
        {
            let filter = tracing_subscriber::EnvFilter::new("info,iroh=warn,noq=warn,iroh_relay=warn");
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(paranoid_android::layer("p2pcore"))
                .try_init();
        }
        #[cfg(not(target_os = "android"))]
        {
            let filter = tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,iroh=warn,noq=warn"));
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
                .try_init();
        }
    });
}
