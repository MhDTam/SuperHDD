//! Read-only inventory for this workstation; never installs or configures a cache.
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use serde_json::json;
    use std::path::Path;
    use superhdd_platform::{cache_runtime, direct_cache::Context};
    use superhdd_storage_control::windows::volume_identity;
    let mut sources = Vec::new();
    for letter in ["D:", "E:"] {
        let facts = volume_identity(letter);
        let eligibility = Context::capture(letter, Path::new(r"M:\SuperHDDCache"), None);
        sources.push(json!({
            "letter":letter,
            "identity":facts.as_ref().ok(),
            "identityError":facts.err(),
            "directSourceEligible":eligibility.is_ok(),
            "eligibilityError":eligibility.err()
        }));
    }
    let parent = volume_identity("M:");
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "scope":"Read-only current-workstation preflight; no driver load, allocation or source writes",
            "readOnly":true,
            "sources":sources,
            "cacheParent":parent.as_ref().ok(),
            "cacheParentError":parent.err(),
            "runtime":cache_runtime::state()
        }))?
    );
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows is required");
    std::process::exit(1);
}
