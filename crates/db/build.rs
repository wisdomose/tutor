// Migrations are embedded at compile time by sqlx::migrate!; rebuild when one is added.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
