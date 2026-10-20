#![no_main]
use libfuzzer_sys::fuzz_target;
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    // Fuzz JSON parsing of auth requests
    if let Ok(s) = std::str::from_utf8(data) {
        let _: Result<Value, _> = serde_json::from_str(s);
    }
    // Fuzz multipart-ish input
    let _ = std::str::from_utf8(data);
});
