// 构建期把仓库根目录的 fork.json / package.json 读进来：发布身份只有一处（§11.8），
// 外壳、打包脚本、CI 都从这里取值，避免 Rust 里再抄一份 appId / 更新源。
use std::{env, fs, path::Path};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("shell-rs 的上一级是仓库根");
    let fork: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("fork.json")).expect("缺 fork.json（发布身份都在里面）"),
    )
    .expect("fork.json 不是合法 JSON");
    let core: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("package.json")).expect("缺 package.json"))
            .expect("package.json 不是合法 JSON");

    let field = |key: &str| {
        fork.get(key)
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string()
    };
    println!("cargo:rustc-env=DSH_APP_ID={}", field("appId"));
    println!("cargo:rustc-env=DSH_PRODUCT_NAME={}", field("productName"));
    println!("cargo:rustc-env=DSH_UPDATE_OWNER={}", field("updateOwner"));
    println!("cargo:rustc-env=DSH_UPDATE_REPO={}", field("updateRepo"));
    println!(
        "cargo:rustc-env=DSH_CORE_VERSION={}",
        core.get("version").and_then(|value| value.as_str()).unwrap_or_default()
    );
    println!("cargo:rerun-if-changed=../fork.json");
    println!("cargo:rerun-if-changed=../package.json");
    println!("cargo:rerun-if-changed=build.rs");
}
