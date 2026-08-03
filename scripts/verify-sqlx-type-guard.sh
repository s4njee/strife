#!/usr/bin/env sh
set -eu

if [ -z "${DATABASE_URL:-}" ]; then
  echo "DATABASE_URL is required" >&2
  exit 2
fi

guard_dir=$(mktemp -d)
trap 'rm -rf "$guard_dir"' EXIT

cat >"$guard_dir/Cargo.toml" <<'EOF'
[package]
name = "strife-sqlx-type-guard"
version = "0.0.0"
edition = "2024"

[dependencies]
sqlx = { version = "0.9.0", default-features = false, features = ["macros", "postgres", "runtime-tokio"] }
tokio = { version = "1.53.1", features = ["macros", "rt"] }
EOF

mkdir "$guard_dir/src"
cat >"$guard_dir/src/main.rs" <<'EOF'
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let _: i64 = sqlx::query_scalar!("SELECT 'wrong-type'::TEXT AS \"value!\"")
        .fetch_one(&pool)
        .await
        .unwrap();
}
EOF

output="$guard_dir/check.log"
if CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target}/sqlx-type-guard" \
  cargo check --manifest-path "$guard_dir/Cargo.toml" >"$output" 2>&1; then
  echo "broken SQLx result type compiled successfully" >&2
  exit 1
fi

if ! grep -q "mismatched types" "$output"; then
  cat "$output" >&2
  echo "SQLx guard failed for an unexpected reason" >&2
  exit 1
fi

echo "SQLx rejected the deliberately broken result type"
