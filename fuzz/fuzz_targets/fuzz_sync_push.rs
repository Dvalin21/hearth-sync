#![no_main]
use libfuzzer_sys::fuzz_target;
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Fuzz sync push payload shape
        let _: Result<Value, _> = serde_json::from_str(s);
        // Fuzz photo metadata fields
        if let Ok(v) = serde_json::from_str::<Value>(s) {
            let _ = v.get("photos");
            let _ = v.get("cursor");
        }
    }
});
