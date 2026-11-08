//! Prism Launcher / MultiMC 实例里的组件。
//!
//! `mmc-pack.json` 按顺序列出组件（`uid` + 版本），例如原版、LWJGL、Forge。每个组件的内容
//! （库、主类、启动参数、tweaker）在 Prism 的元数据服务器上；实例也可以在 `patches/<uid>.json`
//! 里自带一份，覆盖元数据。组件里标了 `"MMC-hint": "local"` 的库放在实例自己的 `libraries/` 下。
//!
//! 这里按 Prism 的规则把组件依次叠起来，合成一个普通的版本 JSON（`arguments` 写法）：
//!
//! - `mainClass`、`minecraftArguments`、`mainJar`、`assetIndex`：后面的组件覆盖前面的。
//! - `+tweakers`、`+jvmArgs`、`+agents`：依次追加。tweaker 变成 `--tweakClass <类>`。
//! - 库：同一个 `group:artifact:classifier` 只留一个，后面的替换前面的，位置不变。
//! - `compatibleJavaMajors`：取最小的作为需要的 Java 版本。

use crate::error::{Error, Result};
use crate::manifest::Coord;
use crate::versions::Loader;
use serde_json::{Map, Value, json};
use std::path::PathBuf;

/// Prism 的元数据服务器。实例没自带补丁的组件从这里取。
pub const META: &str = "https://meta.prismlauncher.org/v1";

/// 合成的结果。
#[derive(Debug, Clone)]
pub struct Built {
    /// 版本 JSON（Mojang 的写法，可以直接写进 profile）。
    pub json: Value,
    /// 实例自带的库：（实例 `libraries/` 下的文件名，放到 `libraries/` 下的相对路径）。
    pub local: Vec<(String, PathBuf)>,
    pub mc: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
}

/// 依次叠起来的状态。
#[derive(Default)]
struct Profile {
    main_class: Option<String>,
    game_args: Option<String>,
    tweakers: Vec<String>,
    jvm: Vec<String>,
    agents: Vec<(String, String)>,
    /// （替换用的键，库）
    libraries: Vec<(String, Value)>,
    main_jar: Option<Value>,
    asset_index: Option<Value>,
    java_majors: Vec<u32>,
    java_name: Option<String>,
    kind: Option<String>,
    release_time: Option<String>,
    local: Vec<(String, PathBuf)>,
}

/// 库的替换键：`group:artifact` 加上 classifier（有的话）。
fn library_key(name: &str) -> String {
    match Coord::parse(name) {
        Ok(c) => match c.classifier {
            Some(classifier) => format!("{}:{}:{classifier}", c.group, c.artifact),
            None => format!("{}:{}", c.group, c.artifact),
        },
        Err(_) => name.to_string(),
    }
}

/// Prism 的库常常不写 `path`：补上。主文件按坐标算；native 的 classifier 文件要带上
/// classifier，否则会算成主文件（或者干脆是空路径，变成 `libraries/` 目录本身）。
fn fill_paths(library: &mut Value, name: &str) {
    let Ok(coord) = Coord::parse(name) else {
        return;
    };
    let path_of = |classifier: Option<&str>| {
        let mut c = coord.clone();
        if let Some(classifier) = classifier {
            c.classifier = Some(classifier.to_string());
        }
        c.rel_path().to_string_lossy().replace('\\', "/")
    };
    let Some(downloads) = library.get_mut("downloads").and_then(Value::as_object_mut) else {
        return;
    };
    if let Some(artifact) = downloads.get_mut("artifact").and_then(Value::as_object_mut)
        && !artifact
            .get("path")
            .and_then(Value::as_str)
            .is_some_and(|p| !p.is_empty())
    {
        artifact.insert("path".into(), Value::String(path_of(None)));
    }
    if let Some(classifiers) = downloads
        .get_mut("classifiers")
        .and_then(Value::as_object_mut)
    {
        for (classifier, artifact) in classifiers.iter_mut() {
            if let Some(artifact) = artifact.as_object_mut()
                && !artifact
                    .get("path")
                    .and_then(Value::as_str)
                    .is_some_and(|p| !p.is_empty())
            {
                artifact.insert("path".into(), Value::String(path_of(Some(classifier))));
            }
        }
    }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

impl Profile {
    fn apply(&mut self, uid: &str, file: &Value) -> Result<()> {
        let unsupported = |what: &str| {
            Err(Error::Unsupported(format!(
                "组件 {uid} 用了 {what}，暂不支持"
            )))
        };
        if let Some(version) = file.get("formatVersion").and_then(Value::as_u64)
            && version != 1
        {
            return unsupported(&format!("格式版本 {version}"));
        }
        for key in ["jarMods", "+jarMods", "mavenFiles", "+mavenFiles"] {
            if file
                .get(key)
                .and_then(Value::as_array)
                .is_some_and(|a| !a.is_empty())
            {
                return unsupported(key);
            }
        }
        if let Some(main) = file.get("mainClass").and_then(Value::as_str)
            && !main.is_empty()
        {
            // Prism 的 Forge 新写法：第一次启动时由 ForgeWrapper 跑安装器。
            if main.to_ascii_lowercase().contains("forgewrapper") {
                return unsupported("ForgeWrapper");
            }
            self.main_class = Some(main.to_string());
        }
        if let Some(args) = file.get("minecraftArguments").and_then(Value::as_str) {
            self.game_args = Some(args.to_string());
        }
        self.tweakers.extend(strings(file.get("+tweakers")));
        self.jvm.extend(strings(file.get("+jvmArgs")));
        if let Some(agents) = file.get("+agents").and_then(Value::as_array) {
            for agent in agents {
                let Some(name) = agent.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let argument = agent
                    .get("argument")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                self.agents.push((name.to_string(), argument.to_string()));
                self.add_library(agent)?;
            }
        }
        for key in ["libraries", "+libraries"] {
            if let Some(libraries) = file.get(key).and_then(Value::as_array) {
                for library in libraries {
                    self.add_library(library)?;
                }
            }
        }
        if let Some(jar) = file.get("mainJar") {
            self.main_jar = Some(jar.clone());
        }
        if let Some(index) = file.get("assetIndex") {
            self.asset_index = Some(index.clone());
        }
        let majors: Vec<u32> = file
            .get("compatibleJavaMajors")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_u64())
                    .map(|v| v as u32)
                    .collect()
            })
            .unwrap_or_default();
        if !majors.is_empty() {
            self.java_majors = majors;
        }
        if let Some(name) = file.get("compatibleJavaName").and_then(Value::as_str) {
            self.java_name = Some(name.to_string());
        }
        if let Some(kind) = file.get("type").and_then(Value::as_str) {
            self.kind = Some(kind.to_string());
        }
        if let Some(time) = file.get("releaseTime").and_then(Value::as_str) {
            self.release_time = Some(time.to_string());
        }
        Ok(())
    }

    /// 加一个库：同一个键的替换掉，位置不变。实例自带的库记下来，安装时从压缩包里取。
    fn add_library(&mut self, library: &Value) -> Result<()> {
        let Some(name) = library.get("name").and_then(Value::as_str) else {
            return Ok(());
        };
        let mut library = library.clone();
        if let Some(object) = library.as_object_mut() {
            let hint = object.remove("MMC-hint");
            let filename = object
                .remove("MMC-filename")
                .and_then(|v| v.as_str().map(str::to_string));
            let absolute = object
                .remove("MMC-absoluteUrl")
                .and_then(|v| v.as_str().map(str::to_string));
            if hint.as_ref().and_then(Value::as_str) == Some("local") {
                let coord = Coord::parse(name)
                    .map_err(|_| Error::Install(format!("看不懂本地库的名字：{name}")))?;
                let relative = coord.rel_path();
                let file = filename.unwrap_or_else(|| {
                    relative
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_default()
                });
                self.local.push((file, relative.clone()));
                // 没有下载地址：文件在导入时放好，不会去网上找。
                object.insert(
                    "downloads".into(),
                    json!({"artifact": {
                        "path": relative.to_string_lossy().replace('\\', "/"),
                        "url": ""
                    }}),
                );
            } else if let Some(url) = absolute {
                let relative = Coord::parse(name)
                    .map(|c| c.rel_path().to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                object.insert(
                    "downloads".into(),
                    json!({"artifact": {"path": relative, "url": url}}),
                );
            }
        }
        fill_paths(&mut library, name);
        let key = library_key(name);
        match self.libraries.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = library,
            None => self.libraries.push((key, library)),
        }
        Ok(())
    }

    fn into_json(self, id: &str) -> Result<Value> {
        let main_class = self
            .main_class
            .ok_or_else(|| Error::Install("组件里没有主类（mainClass）".into()))?;
        let mut game: Vec<Value> = self
            .game_args
            .unwrap_or_default()
            .split_whitespace()
            .map(|s| Value::String(s.to_string()))
            .collect();
        for tweaker in &self.tweakers {
            game.push("--tweakClass".into());
            game.push(Value::String(tweaker.clone()));
        }
        let mut jvm: Vec<Value> = vec!["-Djava.library.path=${natives_directory}".into()];
        for (name, argument) in &self.agents {
            let relative = Coord::parse(name)
                .map(|c| c.rel_path().to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let mut arg = format!("-javaagent:${{library_directory}}/{relative}");
            if !argument.is_empty() {
                arg.push('=');
                arg.push_str(argument);
            }
            jvm.push(Value::String(arg));
        }
        jvm.extend(self.jvm.into_iter().map(Value::String));
        jvm.push("-cp".into());
        jvm.push("${classpath}".into());

        let mut out = Map::new();
        out.insert("id".into(), Value::String(id.to_string()));
        out.insert(
            "type".into(),
            Value::String(self.kind.unwrap_or_else(|| "release".into())),
        );
        if let Some(time) = self.release_time {
            out.insert("releaseTime".into(), Value::String(time));
        }
        out.insert("mainClass".into(), Value::String(main_class));
        out.insert("arguments".into(), json!({"game": game, "jvm": jvm}));
        out.insert(
            "libraries".into(),
            Value::Array(self.libraries.into_iter().map(|(_, l)| l).collect()),
        );
        let artifact = self
            .main_jar
            .as_ref()
            .and_then(|jar| jar.pointer("/downloads/artifact"))
            .cloned()
            .ok_or_else(|| Error::Install("组件里没有游戏本体（mainJar）".into()))?;
        out.insert("downloads".into(), json!({ "client": artifact }));
        if let Some(index) = self.asset_index {
            if let Some(id) = index.get("id").cloned() {
                out.insert("assets".into(), id);
            }
            out.insert("assetIndex".into(), index);
        }
        if let Some(&major) = self.java_majors.iter().min() {
            out.insert(
                "javaVersion".into(),
                json!({
                    "component": self.java_name.unwrap_or_else(|| "java-runtime-gamma".into()),
                    "majorVersion": major
                }),
            );
        }
        Ok(Value::Object(out))
    }
}

/// 按 `components`（`mmc-pack.json` 里的顺序）合成版本 JSON。`patch` 返回实例自带的
/// `patches/<uid>.json`，没有就用 `fetch` 去元数据服务器取。
pub fn build(
    id: &str,
    components: &[(String, String)],
    mut patch: impl FnMut(&str) -> Option<Value>,
    mut fetch: impl FnMut(&str, &str) -> Result<Value>,
) -> Result<Built> {
    let mut profile = Profile::default();
    let mut mc = None;
    let mut loader = (Loader::Vanilla, None);
    for (uid, version) in components {
        match uid.as_str() {
            "net.minecraft" => mc = Some(version.clone()),
            "net.minecraftforge" => loader = (Loader::Forge, Some(version.clone())),
            "net.neoforged" => loader = (Loader::NeoForge, Some(version.clone())),
            "net.fabricmc.fabric-loader" => loader = (Loader::Fabric, Some(version.clone())),
            "org.quiltmc.quilt-loader" => {
                return Err(Error::Unsupported("暂不支持 Quilt 整合包".into()));
            }
            _ => {}
        }
        let file = match patch(uid) {
            Some(file) => file,
            None => fetch(uid, version)?,
        };
        profile.apply(uid, &file)?;
    }
    let mc = mc.ok_or_else(|| Error::Install("整合包没有声明 Minecraft 版本".into()))?;
    let local = std::mem::take(&mut profile.local);
    Ok(Built {
        json: profile.into_json(id)?,
        local,
        mc,
        loader: loader.0,
        loader_version: loader.1,
    })
}

/// 从元数据服务器取一个组件。
pub fn fetch(http: &crate::http::Http, uid: &str, version: &str) -> Result<Value> {
    http.get_json(&format!("{META}/{uid}/{version}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::VersionJson;

    /// 照着一个真实的实例导出包缩小：LWJGL3 换掉原版的 LWJGL2，Forge 换主类，
    /// 再有一个组件加 JVM 参数和一个本地库，最后一个组件改主类。
    fn components() -> Vec<(String, String)> {
        [
            ("org.lwjgl3", "3.4.3"),
            ("net.minecraft", "1.7.10"),
            ("example.earlyclasspath", "3.0.42"),
            ("net.minecraftforge", "10.13.4.1614"),
            ("example.launchargs", "3.0.42"),
        ]
        .iter()
        .map(|(u, v)| (u.to_string(), v.to_string()))
        .collect()
    }

    fn patch(uid: &str) -> Option<Value> {
        Some(match uid {
            "org.lwjgl3" => json!({"formatVersion": 1, "libraries": [
                {"name": "org.lwjgl:lwjgl:3.4.3", "downloads": {"artifact": {"url": "https://x/lwjgl.jar", "sha1": "a"}}},
                {"name": "org.lwjgl:lwjgl-natives-linux:3.4.3", "rules": [{"action": "allow", "os": {"name": "linux"}}],
                 "downloads": {"artifact": {"url": "https://x/lwjgl-natives-linux.jar"}}}
            ]}),
            "net.minecraft" => json!({"formatVersion": 1,
            "mainClass": "net.minecraft.client.main.Main",
            "minecraftArguments": "--username ${auth_player_name} --version ${version_name}",
            "mainJar": {"name": "com.mojang:minecraft:1.7.10:client",
                        "downloads": {"artifact": {"url": "https://x/client.jar", "sha1": "c", "size": 5}}},
            "assetIndex": {"id": "1.7.10", "url": "https://x/1.7.10.json"},
            "compatibleJavaMajors": [17, 21, 25], "compatibleJavaName": "java-runtime-epsilon",
            "libraries": [
                {"name": "com.google.guava:guava:15.0", "downloads": {"artifact": {"url": "https://x/guava15.jar"}}},
                {"name": "com.mojang:authlib:1.5.21", "downloads": {"artifact": {"url": "https://x/authlib.jar"}}}
            ]}),
            "example.earlyclasspath" => json!({"formatVersion": 1,
                "+jvmArgs": ["-Djava.system.class.loader=example.Loader", "--add-opens", "java.base/java.lang=ALL-UNNAMED"],
                "libraries": [{"name": "com.example:early:3.0.42:patches", "MMC-hint": "local"}]}),
            "net.minecraftforge" => json!({"formatVersion": 1,
            "mainClass": "example.bootstrap.Main",
            "+tweakers": ["cpw.mods.fml.common.launcher.FMLTweaker"],
            "libraries": [
                {"name": "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10:universal",
                 "downloads": {"artifact": {"url": "https://x/forge.jar"}}},
                {"name": "com.google.guava:guava:17.0", "downloads": {"artifact": {"url": "https://x/guava17.jar"}}}
            ]}),
            "example.launchargs" => {
                json!({"formatVersion": 1, "mainClass": "example.bootstrap.MainOnFirstThread"})
            }
            _ => return None,
        })
    }

    #[test]
    fn components_stack_up_like_prism_does() {
        let built = build("pack", &components(), patch, |uid, _| {
            panic!("{uid} 不该去网上取")
        })
        .unwrap();
        assert_eq!(built.mc, "1.7.10");
        assert_eq!(built.loader, Loader::Forge);
        assert_eq!(built.loader_version.as_deref(), Some("10.13.4.1614"));
        assert_eq!(
            built.local,
            vec![(
                "early-3.0.42-patches.jar".to_string(),
                PathBuf::from("com/example/early/3.0.42/early-3.0.42-patches.jar")
            )]
        );
        let json = &built.json;
        // 后面的组件改主类。
        assert_eq!(json["mainClass"], "example.bootstrap.MainOnFirstThread");
        // guava 17 替换 guava 15，位置不变。
        let names: Vec<&str> = json["libraries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "org.lwjgl:lwjgl:3.4.3",
                "org.lwjgl:lwjgl-natives-linux:3.4.3",
                "com.google.guava:guava:17.0",
                "com.mojang:authlib:1.5.21",
                "com.example:early:3.0.42:patches",
                "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10:universal",
            ]
        );
        let game: Vec<&str> = json["arguments"]["game"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            &game[game.len() - 2..],
            ["--tweakClass", "cpw.mods.fml.common.launcher.FMLTweaker"]
        );
        let jvm: Vec<&str> = json["arguments"]["jvm"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(jvm.contains(&"-Djava.system.class.loader=example.Loader"));
        assert_eq!(jvm[0], "-Djava.library.path=${natives_directory}");
        assert_eq!(json["javaVersion"]["majorVersion"], 17);
        assert_eq!(json["downloads"]["client"]["url"], "https://x/client.jar");
        assert_eq!(json["assets"], "1.7.10");

        // 我们自己的启动和安装能读懂合成的 JSON。
        let version: VersionJson = serde_json::from_value(json.clone()).unwrap();
        let local = version
            .libraries
            .iter()
            .find(|l| l.name.contains("early"))
            .unwrap();
        assert_eq!(local.artifact().unwrap().1.url, "");
    }

    #[test]
    fn native_classifiers_get_their_own_paths() {
        let mut profile = Profile::default();
        profile
            .add_library(&json!({
                "name": "net.java.jinput:jinput-platform:2.0.5",
                "natives": {"linux": "natives-linux"},
                "downloads": {
                    "artifact": {"url": "https://x/a.jar"},
                    "classifiers": {"natives-linux": {"url": "https://x/n.jar", "sha1": "s"}}
                }
            }))
            .unwrap();
        let library = &profile.libraries[0].1;
        assert_eq!(
            library["downloads"]["artifact"]["path"],
            "net/java/jinput/jinput-platform/2.0.5/jinput-platform-2.0.5.jar"
        );
        assert_eq!(
            library["downloads"]["classifiers"]["natives-linux"]["path"],
            "net/java/jinput/jinput-platform/2.0.5/jinput-platform-2.0.5-natives-linux.jar"
        );
    }

    #[test]
    fn missing_patches_come_from_the_meta_server_and_forgewrapper_is_refused() {
        let mut asked = Vec::new();
        let built = build(
            "plain",
            &[("net.minecraft".to_string(), "1.7.10".to_string())],
            |_| None,
            |uid, version| {
                asked.push(format!("{uid}/{version}"));
                Ok(patch("net.minecraft").unwrap())
            },
        )
        .unwrap();
        assert_eq!(asked, ["net.minecraft/1.7.10"]);
        assert_eq!(built.loader, Loader::Vanilla);

        let err = build(
            "modern",
            &[
                ("net.minecraft".to_string(), "1.20.1".to_string()),
                ("net.minecraftforge".to_string(), "47.3.0".to_string()),
            ],
            |uid| (uid == "net.minecraft").then(|| patch("net.minecraft").unwrap()),
            |_, _| Ok(json!({"formatVersion": 1, "mainClass": "io.github.zekerzhayard.forgewrapper.installer.Main"})),
        )
        .unwrap_err();
        assert!(err.to_string().contains("ForgeWrapper"));
    }
}
