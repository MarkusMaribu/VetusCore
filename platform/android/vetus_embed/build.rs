fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("android") {
        // Android 15+ devices can use 16 KB pages. NDK r26's driver still
        // defaults shared objects to 4 KB; the last max-page-size wins.
        println!("cargo:rustc-link-arg=-Wl,-z,max-page-size=16384");
        println!("cargo:rustc-link-arg=-Wl,-z,common-page-size=16384");
    }
}
