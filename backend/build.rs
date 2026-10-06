const FRONTEND_DIST: &str = "../frontend/dist";

fn main() {
    // `sqlx::migrate!` embeds migrations at compile time; rebuild when they change.
    println!("cargo:rerun-if-changed=src/db/migrations");

    // `#[derive(RustEmbed)]` requires the folder to exist at compile time; an empty
    // one keeps backend-only builds and tests working before `npm run build`.
    // Rebuild when the frontend build output changes so the binary embeds it.
    std::fs::create_dir_all(FRONTEND_DIST).expect("create frontend dist dir");
    println!("cargo:rerun-if-changed={FRONTEND_DIST}");
}
