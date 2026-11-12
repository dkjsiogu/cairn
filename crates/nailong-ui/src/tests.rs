//! 界面层的单元测试。

use crate::play::Recovery;
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

#[test]
fn prefs_keep_unknown_keys_and_reject_broken_values() {
    let root = temp_root("prefs");
    std::fs::create_dir_all(root.dir()).unwrap();
    std::fs::write(
        root.dir().join("ui.json"),
        r#"{"theme":"light","zoom":1.13,"window":[-5,99999],"future_key":42,"gc":"zgc"}"#,
    )
    .unwrap();
    let mut prefs = UiPrefs::load(&root);
    assert_eq!(prefs.theme, t::Preference::Light);
    // 落在最近的一档；坏掉的窗口大小不用。
    assert_eq!(prefs.zoom, 1.1);
    assert_eq!(prefs.window, None);
    prefs.selected = Some("生存".into());
    prefs.window = Some([1280.0, 800.0]);
    prefs.save(&root);
    let text = std::fs::read_to_string(root.dir().join("ui.json")).unwrap();
    // 新版本写的键旧版本不认识，旧版本写回去时也不能丢。
    assert!(text.contains("future_key"), "{text}");
    let again = UiPrefs::load(&root);
    assert_eq!(again.selected.as_deref(), Some("生存"));
    assert_eq!(again.window, Some([1280.0, 800.0]));
    assert_eq!(again.gc, tuning::GcChoice::Zgc);
    let _ = std::fs::remove_dir_all(root.dir());
}

fn row(id: &str, mc: &str, loader: LoaderKind, version: Option<&str>, pack: Option<&str>) -> Row {
    Row {
        id: id.into(),
        profile: id.into(),
        label: String::new(),
        mc: mc.into(),
        loader,
        loader_version: version.map(str::to_string),
        game_dir: PathBuf::new(),
        pack: pack.map(str::to_string),
        health: Default::default(),
    }
}

#[test]
fn home_titles_show_names_people_chose() {
    use crate::pages::home::home_title;
    let neo = Some("21.1.209");
    // 默认名字和别的启动器的写法：只写游戏版本。
    for id in [
        "1.21.1",
        "1.21.1 NeoForge",
        "neoforge-21.1.209",
        "1.21.1-neoforge-21.1.209",
    ] {
        assert_eq!(
            home_title(&row(id, "1.21.1", LoaderKind::NeoForge, neo, None)),
            ("1.21.1".to_string(), false),
            "{id}"
        );
    }
    assert_eq!(
        home_title(&row(
            "fabric-loader-0.16.10-1.21.1",
            "1.21.1",
            LoaderKind::Fabric,
            Some("0.16.10"),
            None
        )),
        ("1.21.1".to_string(), false)
    );
    // 起过名字的、整合包：原样显示。
    assert_eq!(
        home_title(&row(
            "生存 带朋友",
            "1.21.1",
            LoaderKind::NeoForge,
            neo,
            None
        )),
        ("生存 带朋友".to_string(), true)
    );
    assert_eq!(
        home_title(&row(
            "x",
            "1.20.1",
            LoaderKind::Forge,
            None,
            Some("模拟大都市")
        )),
        ("模拟大都市".to_string(), true)
    );
}

#[test]
fn recovery_points_at_the_first_change_after_the_last_good_game() {
    let root = temp_root("recovery");
    let dir = root.dir().join("game");
    std::fs::create_dir_all(dir.join("mods")).unwrap();
    std::fs::write(dir.join("mods/a.jar"), b"a").unwrap();
    let first = rollback::backup(&dir, "安装 A 前").unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(dir.join("mods/b.jar"), b"b").unwrap();
    rollback::backup(&dir, "安装 B 前").unwrap();
    // 正常玩过的那一局在两次改动之前：退回到第一次改动之前。
    let found = Recovery::find("v", &dir, first.created_at - 1).unwrap();
    assert_eq!(found.point.id, first.id);
    assert_eq!(found.changes, 2);
    // 改动都在那局之前：没什么可退的。
    assert!(Recovery::find("v", &dir, u64::MAX / 2).is_none());
    // 从没正常玩过：不知道退到哪，不给。
    assert!(Recovery::find("v", &dir, 0).is_none());
    let _ = std::fs::remove_dir_all(root.dir());
}
