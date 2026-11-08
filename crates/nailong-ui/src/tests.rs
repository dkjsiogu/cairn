//! 界面层的单元测试。

use crate::prelude::*;

fn temp_root(tag: &str) -> Root {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    Root::new(std::env::temp_dir().join(format!("nailong-ui-{tag}-{}-{nanos}", std::process::id())))
}

#[test]
fn windows_browser_arg_keeps_full_oauth_query() {
    let url = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize?client_id=x&scope=XboxLive.signin%20offline_access&state=abc";
    let args = browser_launch_args(url);
    assert_eq!(args[1], url);
    assert!(args[1].contains("&scope=XboxLive.signin"));
}

#[test]
fn login_errors_are_actionable_not_raw_aadsts() {
    let scope =
        friendly_login_error("微软登录失败：invalid_request（AADSTS900144: scope is missing）");
    assert!(scope.contains("更新"));
    assert!(!scope.contains("AADSTS"));
    assert_eq!(
        friendly_login_error("微软登录失败：invalid_client"),
        "启动器的微软登录配置暂时不可用，请稍后重试"
    );
    assert_eq!(friendly_login_error("已取消登录"), "已取消登录");
}

#[test]
fn version_rows_read_the_cairn_record_or_guess() {
    let root = temp_root("info");
    let made = root.version_dir("生存");
    std::fs::create_dir_all(&made).unwrap();
    std::fs::write(
        made.join("生存.json"),
        r#"{"id":"生存","cairn":{"mc":"1.21.1","loader":"neoforge","loaderVersion":"21.1.209",
            "profile":"neoforge-21.1.209","pack":{"name":"城市","format":"mrpack"}}}"#,
    )
    .unwrap();
    assert_eq!(
        version_info(&root, "生存"),
        (
            "1.21.1".to_string(),
            LoaderKind::NeoForge,
            Some("21.1.209".to_string()),
            Some("城市".to_string())
        )
    );
    // 别的启动器装的：没有记录，按 id 猜。
    let other = root.version_dir("fabric-loader-0.16.10-1.21.1");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(
        other.join("fabric-loader-0.16.10-1.21.1.json"),
        r#"{"id":"fabric-loader-0.16.10-1.21.1"}"#,
    )
    .unwrap();
    let (mc, loader, version, pack) = version_info(&root, "fabric-loader-0.16.10-1.21.1");
    assert_eq!(
        (mc.as_str(), loader, version.as_deref(), pack),
        ("1.21.1", LoaderKind::Fabric, Some("0.16.10"), None)
    );
    let _ = std::fs::remove_dir_all(root.dir());
}

#[test]
fn removal_is_direct_child_only() {
    let root = temp_root("remove");
    std::fs::create_dir_all(root.versions().join("old")).unwrap();
    std::fs::write(root.versions().join("old").join("x"), b"x").unwrap();
    root.remove_version("old").unwrap();
    assert!(!root.versions().join("old").exists());
    assert!(root.remove_version("../outside").is_err());
    assert!(root.remove_version(".").is_err());
    let _ = std::fs::remove_dir_all(root.dir());
}
