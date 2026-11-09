//! 界面用到的数据：版本行、导航、后台任务种类、待确认的安装，以及从磁盘读版本信息的纯函数。

use crate::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LoaderKind {
    Vanilla,
    Fabric,
    NeoForge,
    Forge,
}

impl LoaderKind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Vanilla => "原版",
            Self::Fabric => "Fabric",
            Self::NeoForge => "NeoForge",
            Self::Forge => "Forge",
        }
    }

    /// core 里的写法。
    pub(crate) fn core(self) -> versions::Loader {
        match self {
            Self::Vanilla => versions::Loader::Vanilla,
            Self::Fabric => versions::Loader::Fabric,
            Self::NeoForge => versions::Loader::NeoForge,
            Self::Forge => versions::Loader::Forge,
        }
    }

    pub(crate) fn from_core(loader: versions::Loader) -> Self {
        match loader {
            versions::Loader::Vanilla => Self::Vanilla,
            versions::Loader::Fabric => Self::Fabric,
            versions::Loader::NeoForge => Self::NeoForge,
            versions::Loader::Forge => Self::Forge,
        }
    }

    pub(crate) fn rank(self) -> u8 {
        match self {
            Self::Fabric => 0,
            Self::NeoForge => 1,
            Self::Forge => 2,
            Self::Vanilla => 3,
        }
    }
}

pub(crate) fn mod_loader(kind: LoaderKind) -> Option<mods::Loader> {
    match kind {
        LoaderKind::Fabric => Some(mods::Loader::Fabric),
        LoaderKind::NeoForge => Some(mods::Loader::NeoForge),
        LoaderKind::Forge => Some(mods::Loader::Forge),
        LoaderKind::Vanilla => None,
    }
}

/// 「安装到」的完整说明：游戏版本 + 加载器 + 加载器版本。
pub(crate) fn target_label(row: &Row) -> String {
    match row.loader_version.as_deref() {
        Some(version) => format!("{} · {} {}", row.mc, row.loader.name(), version),
        None => format!("{} · {}", row.mc, row.loader.name()),
    }
}

/// 恢复点里要带上的版本文件：版本 JSON 和客户端 jar 就在游戏目录（版本文件夹）里。
pub(crate) fn loader_state_files(
    root: &Root,
    row: &Row,
    game_dir: &std::path::Path,
) -> Vec<String> {
    let _ = (root, game_dir);
    vec![
        format!("{}.json", row.profile),
        format!("{}.jar", row.profile),
    ]
}

/// 一个版本：`versions/<名字>/` 一个文件夹（见 docs/design/27-version-folders.md）。
#[derive(Clone)]
pub(crate) struct Row {
    /// 版本名，也就是文件夹名。
    pub(crate) id: String,
    /// 和 `id` 一样；启动、补全、恢复点都按它找版本 JSON。
    pub(crate) profile: String,
    pub(crate) label: String,
    pub(crate) mc: String,
    pub(crate) loader: LoaderKind,
    pub(crate) loader_version: Option<String>,
    /// 游戏目录，就是版本文件夹。
    pub(crate) game_dir: PathBuf,
    /// 整合包的名字；不是整合包时为 None。
    pub(crate) pack: Option<String>,
    pub(crate) health: health::Health,
}

#[derive(Clone)]
pub(crate) enum RepairTarget {
    Loader {
        mc: String,
        loader: LoaderKind,
        version: String,
        /// 装到哪个版本（文件夹名）。
        name: String,
    },
    Modpack(PathBuf),
}

impl RepairTarget {
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Loader {
                mc,
                loader,
                version,
                ..
            } => {
                if *loader == LoaderKind::Vanilla {
                    mc.clone()
                } else {
                    format!("{mc} · {} {version}", loader.name())
                }
            }
            Self::Modpack(path) => path
                .file_name()
                .map(|s| format!("整合包 {}", s.to_string_lossy()))
                .unwrap_or_else(|| "整合包".to_string()),
        }
    }
}

/// 正在跑的后台任务。同一时间只有一个。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Task {
    /// 安装或修复版本、导入整合包。
    Install,
    /// 下载 Mod、资源包、光影或更新它们。
    Mods,
    /// 游戏在运行。
    Launch,
    /// 测一下这台电脑：后台跑模拟负载，不开游戏窗口。
    Bench,
    /// 下载这个版本需要的 Java，下完接着启动。
    Java,
    /// 导入或删除本地的内容文件（先建恢复点）。不占整页，页面照常能看。
    Files,
}

impl Task {
    /// 这些任务进行时整页显示进度，别的页面先锁住。
    pub(crate) fn shows_progress(self) -> bool {
        matches!(self, Self::Install | Self::Mods | Self::Bench | Self::Java)
    }
}

/// 已安装列表的状态筛选。状态数字本身就是入口：点「3 个已禁用」就只看这 3 个。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StatusFilter {
    All,
    Enabled,
    Disabled,
    Updates,
    Duplicates,
}

impl StatusFilter {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Enabled => "已启用",
            Self::Disabled => "已禁用",
            Self::Updates => "可更新",
            Self::Duplicates => "重复",
        }
    }
}

/// 命令行预览：给人看的逐行写法和能复制的整条命令，或者生成失败的原因。
pub(crate) type CommandPreview = Result<(Vec<String>, String), String>;

/// 启动参数窗口的状态：在改哪个版本、草稿，以及命令行预览。
pub(crate) struct LaunchEdit {
    pub(crate) row_id: String,
    pub(crate) draft: VersionLaunch,
    /// 自定义窗口大小的输入框。
    pub(crate) width: String,
    pub(crate) height: String,
    /// 预览对应的设置；和当前草稿不一样就重新算。
    pub(crate) preview_key: String,
    pub(crate) preview_rx: Option<mpsc::Receiver<CommandPreview>>,
    pub(crate) preview: Option<CommandPreview>,
    pub(crate) copied_at: Option<f64>,
}

/// 拖进来的文件分好类之后要做的事。
/// 后台导入或删除内容的结果。
#[derive(Default)]
pub(crate) struct ContentDone {
    /// 是在内容页上做的（提示写在内容页）。
    pub(crate) on_content: bool,
    pub(crate) names: Vec<String>,
    pub(crate) kinds: Vec<mods::ContentKind>,
    /// 新导入的文件名，列表里标「新」。
    pub(crate) recent: Vec<String>,
    pub(crate) skipped: Vec<String>,
    pub(crate) problems: Vec<String>,
    /// 删除完成时的提示。
    pub(crate) removed: Option<String>,
}

#[derive(Default)]
pub(crate) struct DropPlan {
    /// 导入到哪个版本、哪个游戏目录。
    pub(crate) target: Option<(Row, PathBuf)>,
    pub(crate) items: Vec<(PathBuf, mods::ContentKind)>,
    pub(crate) packs: Vec<PathBuf>,
    pub(crate) unknown: Vec<PathBuf>,
}

/// 侧边栏的四个一级入口。玩家只需要记住这四个地方。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Nav {
    Home,
    Versions,
    Content,
    Saves,
    Shots,
}

impl Nav {
    pub(crate) const ALL: [Nav; 5] = [
        Nav::Home,
        Nav::Versions,
        Nav::Content,
        Nav::Saves,
        Nav::Shots,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Home => "启动台",
            Self::Versions => "版本",
            Self::Content => "内容",
            Self::Saves => "存档",
            Self::Shots => "截图",
        }
    }

    pub(crate) fn hint(self) -> &'static str {
        match self {
            Self::Home => "启动、账户与最近动态",
            Self::Versions => "安装、修复与删除版本",
            Self::Content => "Mod、资源包、光影",
            Self::Saves => "世界与快照备份",
            Self::Shots => "时间、地点、人物",
        }
    }
}

/// 浏览页的一排内容：热门 / 最新。
pub(crate) struct Shelf {
    pub(crate) title: &'static str,
    pub(crate) sort: mods::Sort,
    pub(crate) projects: Vec<mods::Project>,
}

/// 安装确认里列出的一个前置。
#[derive(Clone)]
pub(crate) struct ReviewDependency {
    pub(crate) name: String,
    pub(crate) required: bool,
    pub(crate) missing: bool,
}

/// 更新确认里的一个项目。
#[derive(Clone)]
pub(crate) struct ReviewUpdate {
    pub(crate) name: String,
    pub(crate) from: String,
    pub(crate) to: String,
}

/// 写入磁盘之前给玩家看的最后一张清单。
#[derive(Clone)]
pub(crate) struct InstallReview {
    pub(crate) name: String,
    pub(crate) kind: mods::ContentKind,
    pub(crate) target: String,
    pub(crate) path: String,
    pub(crate) version: String,
    pub(crate) size: Option<u64>,
    pub(crate) dependencies: Vec<ReviewDependency>,
    pub(crate) updates: Vec<ReviewUpdate>,
    pub(crate) quick: bool,
    pub(crate) update: bool,
}

#[derive(Clone)]
pub(crate) enum InstallRequest {
    Quick(mods::Project),
    Detail,
    Update(Vec<installed::UpdateCandidate>),
}

pub(crate) struct PendingInstall {
    pub(crate) review: InstallReview,
    pub(crate) request: InstallRequest,
}

/// 启动台一帧只读一次磁盘；切页面或内容变化时再重建。
#[derive(Clone)]
pub(crate) struct HomeCache {
    pub(crate) row_id: String,
    pub(crate) mods: usize,
    pub(crate) packs: usize,
    pub(crate) latest_world: Option<saves::World>,
    /// 每个 Mod 的文件名和是否启用，状态圆环一个点对应一个。
    pub(crate) mod_entries: Vec<(String, String, bool)>,
    pub(crate) snapshots: usize,
    pub(crate) latest_snapshot: Option<u64>,
}

pub(crate) type CfCategoryFetch = (mods::ContentKind, Result<Vec<mods::CurseCategory>, String>);

pub(crate) fn project_key(project: &mods::Project) -> String {
    format!("{:?}:{}", project.source, project.id)
}

/// 一个版本的负载，以及它的实测记录和记录是否仍然有效。
/// 启动时会用的负载和 Java，以及这个版本玩过之后学到的内存。
pub(crate) type TuningEntry = (tuning::Workload, java::JavaRuntime, Option<tuning::Learned>);

pub(crate) fn available(cat: &catalog::Catalog, kind: LoaderKind, mc: &str) -> bool {
    match kind {
        LoaderKind::Vanilla => true,
        LoaderKind::Fabric => catalog::fabric_supported(mc) && !cat.fabric.is_empty(),
        LoaderKind::NeoForge => !cat.neoforge_for(mc).is_empty(),
        LoaderKind::Forge => !cat.forge_for(mc).is_empty(),
    }
}

pub(crate) fn preferred_kind(cat: &catalog::Catalog, mc: &str) -> LoaderKind {
    for kind in [
        LoaderKind::Fabric,
        LoaderKind::NeoForge,
        LoaderKind::Forge,
        LoaderKind::Vanilla,
    ] {
        if available(cat, kind, mc) {
            return kind;
        }
    }
    LoaderKind::Vanilla
}

pub(crate) fn preferred_fabric(cat: &catalog::Catalog) -> String {
    cat.fabric.first().cloned().unwrap_or_default()
}

pub(crate) fn preferred_neoforge(cat: &catalog::Catalog, mc: &str) -> String {
    let list = cat.neoforge_for(mc);
    if mc == "1.21.1" && list.iter().any(|v| v == "21.1.209") {
        return "21.1.209".to_string();
    }
    list.first().cloned().unwrap_or_default()
}

pub(crate) fn preferred_forge(cat: &catalog::Catalog, mc: &str) -> String {
    cat.forge_for(mc).first().cloned().unwrap_or_default()
}

/// 一个版本是什么：MC 版本、加载器、加载器版本、整合包名。Cairn 装的读 JSON 里的 `cairn`
/// 记录，别的启动器装的按内容猜。
pub(crate) fn version_info(
    root: &Root,
    name: &str,
) -> (String, LoaderKind, Option<String>, Option<String>) {
    match versions::meta(root, name) {
        Some(meta) => (
            meta.mc,
            LoaderKind::from_core(meta.loader),
            meta.loader_version,
            meta.pack.map(|pack| pack.name),
        ),
        None => {
            let (mc, loader, version) = versions::detect(root, name);
            (mc, LoaderKind::from_core(loader), version, None)
        }
    }
}

pub(crate) fn mc_key(v: &str) -> Vec<u64> {
    v.split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect()
}

/// 这台电脑：找到的 Java、配置，以及「测一下这台电脑」的结论。
pub(crate) struct Env {
    pub(crate) javas: Vec<java::JavaRuntime>,
    /// 在启动参数里手动选过、但自动检测没找到的 Java。
    pub(crate) extra_javas: Vec<java::JavaRuntime>,
    pub(crate) java_ok: bool,
    pub(crate) java_label: Option<String>,
    pub(crate) machine: tuning::Machine,
    /// 「测一下这台电脑」的结果，所有版本共用。
    pub(crate) bench: Option<bench::Store>,
}
