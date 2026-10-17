// Embeds the application manifest. The defaults are the ones the C# build set
// by hand in app.manifest: asInvoker (an elevated hook cannot see input bound
// for unelevated windows), per-monitor-v2 DPI awareness, long paths, and
// Common Controls v6 so the key dialog looks like Windows 11 rather than 95.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_manifest::embed_manifest(embed_manifest::new_manifest("dictate"))
            .expect("unable to embed the application manifest");
    }

    println!("cargo:rerun-if-changed=build.rs");
}
