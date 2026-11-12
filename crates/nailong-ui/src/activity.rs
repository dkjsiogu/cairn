//! 后台任务给人看的样子：现在在做哪一步、做完了哪几步、刚下载了哪些文件、从哪下、多快、还要多久。
//!
//! 后台线程只管往外报 `Progress`；这里把一串进度攒成玩家关心的东西。下载页和更新弹窗都用它。

use crate::prelude::*;
use std::collections::VecDeque;

/// 最近下载的文件留几个。
const RECENT: usize = 7;
/// 用最近几秒的数据算速度：太短会跳，太长反应慢。
const SPEED_WINDOW: Duration = Duration::from_secs(3);

pub(crate) struct Step {
    pub(crate) name: String,
    started: Instant,
    /// 做完了用了多久；正在做的为 None。
    pub(crate) took: Option<Duration>,
    /// 这一步最近一次报的进度。安装时几步会同时进行（比如资源和运行库），各记各的。
    last: Progress,
}

impl Step {
    /// 有数量（文件数或字节数）的步骤；只报了名字的步骤没有。
    fn counted(&self) -> bool {
        self.last.files_total > 0 || self.last.bytes_total > 0
    }

    /// 这一步做了多少（0–1）：按字节，没有字节按文件数；只报了名字的，做完算 1。
    fn fraction(&self) -> f32 {
        let p = &self.last;
        if p.bytes_total > 0 {
            (p.bytes_done.min(p.bytes_total) as f64 / p.bytes_total as f64) as f32
        } else if p.files_total > 0 {
            (p.files_done.min(p.files_total) as f64 / p.files_total as f64) as f32
        } else if self.took.is_some() {
            1.0
        } else {
            0.0
        }
    }

    /// 进行中的步骤右边写的进度：下载写百分比，按文件数的写「3/9」。
    pub(crate) fn detail(&self) -> Option<String> {
        let p = &self.last;
        if self.took.is_some() {
            None
        } else if p.bytes_total > 0 {
            Some(format!("{:.0}%", self.fraction() * 100.0))
        } else if p.files_total > 1 {
            Some(format!(
                "{}/{}",
                p.files_done.min(p.files_total),
                p.files_total
            ))
        } else {
            None
        }
    }

    fn complete(&self) -> bool {
        let p = &self.last;
        !p.indeterminate
            && if p.bytes_total > 0 {
                p.bytes_done >= p.bytes_total
            } else {
                p.files_total > 0 && p.files_done >= p.files_total
            }
    }
}

pub(crate) struct Activity {
    /// 例如「1.21.1 · NeoForge 21.1.264」。
    pub(crate) title: String,
    /// 最近一次报的进度。
    pub(crate) progress: Progress,
    pub(crate) steps: Vec<Step>,
    /// 最近下载的文件，最新的在前。
    pub(crate) recent: VecDeque<String>,
    /// 最近一次下载用的主机。
    pub(crate) source: Option<String>,
    /// （时间，所有步骤加起来下了多少字节）。换步骤时不清空，速度不会忽高忽低。
    samples: VecDeque<(Instant, u64)>,
    /// 显示的总进度只往前走。
    shown: f32,
    /// 预计会有的几步：（步骤名里包含的字，占总进度的多少）。还没开始的按 0 计，
    /// 这样先做完的小步骤不会让总进度一下子到 100%。没列出的步骤各占 1。
    expected: Vec<(&'static str, f32)>,
    started: Instant,
}

impl Activity {
    pub(crate) fn new(title: impl Into<String>, stage: impl Into<String>) -> Self {
        let mut activity = Self {
            title: title.into(),
            progress: Progress::default(),
            steps: Vec::new(),
            recent: VecDeque::new(),
            source: None,
            samples: VecDeque::new(),
            shown: 0.0,
            expected: Vec::new(),
            started: Instant::now(),
        };
        activity.update(Progress::stage(stage));
        activity
    }

    /// 说明这个任务大概有哪几步、各占多少（见 [`plan`] 里的几种）。
    pub(crate) fn expect(mut self, groups: &[(&'static str, f32)]) -> Self {
        self.expected = groups.to_vec();
        self.shown = 0.0;
        self
    }

    /// 这一步以前报过没有。
    pub(crate) fn has_step(&self, name: &str) -> bool {
        self.steps.iter().any(|step| step.name == name)
    }

    pub(crate) fn update(&mut self, p: Progress) {
        let now = Instant::now();
        let index = match self.steps.iter().position(|step| step.name == p.stage) {
            Some(index) => {
                self.steps[index].last = p.clone();
                index
            }
            None => {
                // 新的一步开始：前面只报了名字、没有数量的步骤（读取版本信息之类）算做完了。
                for step in &mut self.steps {
                    if step.took.is_none() && !step.counted() {
                        step.took = Some(now - step.started);
                    }
                }
                self.steps.push(Step {
                    name: p.stage.clone(),
                    started: now,
                    took: None,
                    last: p.clone(),
                });
                self.steps.len() - 1
            }
        };
        let step = &mut self.steps[index];
        if step.took.is_none() && step.complete() {
            step.took = Some(now - step.started);
        }
        if let Some(current) = &p.current
            && self.recent.front() != Some(current)
        {
            self.recent.retain(|name| name != current);
            self.recent.push_front(current.clone());
            self.recent.truncate(RECENT);
        }
        if p.source.is_some() {
            self.source = p.source.clone();
        }
        let (done, total) = self.bytes();
        if total > 0 || done > 0 {
            self.samples.push_back((now, done));
            while self
                .samples
                .front()
                .is_some_and(|(at, _)| now - *at > SPEED_WINDOW)
                && self.samples.len() > 2
            {
                self.samples.pop_front();
            }
        }
        self.progress = p;
        if let Some(overall) = self.overall_now() {
            self.shown = self.shown.max(overall);
        }
    }

    /// 所有步骤加起来：已下载、总共多少字节。
    pub(crate) fn bytes(&self) -> (u64, u64) {
        self.steps
            .iter()
            .filter(|step| step.last.bytes_total > 0)
            .fold((0, 0), |(done, total), step| {
                (
                    done + step.last.bytes_done.min(step.last.bytes_total),
                    total + step.last.bytes_total,
                )
            })
    }

    /// 所有步骤加起来：已完成、总共多少个文件（只算报了文件数的步骤）。
    pub(crate) fn files(&self) -> (u64, u64) {
        self.steps
            .iter()
            .filter(|step| step.last.files_total > 1)
            .fold((0, 0), |(done, total), step| {
                (
                    done + step.last.files_done.min(step.last.files_total),
                    total + step.last.files_total,
                )
            })
    }

    /// 总进度（0–1）。每组预计的步骤按权重算，组里的步骤取平均；没列出的步骤各占 1。
    /// 「安装完成」这类收尾步骤出现时直接是 1。什么都不知道时为 None。
    fn overall_now(&self) -> Option<f32> {
        // 「前置|依赖」：任一个字出现在步骤名里就算这一组。
        let group_of = |name: &str| {
            self.expected
                .iter()
                .position(|(key, _)| key.split('|').any(|k| name.contains(k)))
        };
        // 最后一步是「安装完成」这类收尾，而且预计的几组都开始过了：就是做完了。
        // 整合包里装完加载器也会报一次「安装完成」，那时整合包文件还没开始，不算。
        let all_seen = (0..self.expected.len()).all(|index| {
            self.steps
                .iter()
                .any(|step| step.counted() && group_of(&step.name) == Some(index))
        });
        if all_seen
            && self.steps.last().is_some_and(|step| {
                (step.name.ends_with("完成") || step.name.ends_with("已装好")) && step.complete()
            })
        {
            return Some(1.0);
        }
        let mut total = 0.0f32;
        let mut done = 0.0f32;
        for (index, (_, weight)) in self.expected.iter().enumerate() {
            // 组里只算有数量的步骤；只报了名字的（比如「准备下载 Java」）不算。
            let fractions: Vec<f32> = self
                .steps
                .iter()
                .filter(|step| step.counted() && group_of(&step.name) == Some(index))
                .map(Step::fraction)
                .collect();
            total += weight;
            if !fractions.is_empty() {
                done += weight * fractions.iter().sum::<f32>() / fractions.len() as f32;
            }
        }
        let mut counted_any = !self.expected.is_empty();
        for step in self
            .steps
            .iter()
            .filter(|step| group_of(&step.name).is_none())
        {
            counted_any |= step.counted();
            total += 1.0;
            done += step.fraction();
        }
        (counted_any && total > 0.0).then(|| (done / total).clamp(0.0, 1.0))
    }

    /// 显示用的总进度：不会往回退。还不知道数量时为 None。
    pub(crate) fn overall(&self) -> Option<f32> {
        self.overall_now().map(|_| self.shown)
    }

    /// 还没做完的步骤，最近开始的在后面。
    pub(crate) fn active(&self) -> Vec<&str> {
        self.steps
            .iter()
            .filter(|step| step.took.is_none())
            .map(|step| step.name.as_str())
            .collect()
    }

    /// 每秒字节数（所有步骤加起来）；数据不够时为 None。
    pub(crate) fn speed(&self) -> Option<f64> {
        let (first, last) = (self.samples.front()?, self.samples.back()?);
        let secs = (last.0 - first.0).as_secs_f64();
        (secs >= 0.5).then(|| last.1.saturating_sub(first.1) as f64 / secs)
    }

    /// 总共还要多少秒：按到现在用的时间和总进度推算，所以也算上 processor 这类不下载的步骤。
    /// 刚开始、进度太少时不估。
    pub(crate) fn eta(&self) -> Option<f64> {
        let overall = self.overall()? as f64;
        let elapsed = self.started.elapsed().as_secs_f64();
        ((0.03..1.0).contains(&overall) && elapsed >= 2.0)
            .then(|| elapsed * (1.0 - overall) / overall)
    }

    /// 下载源的名字：玩家认得的是「Mojang 官方」「BMCLAPI 镜像」，不是主机名。
    pub(crate) fn source_label(&self) -> Option<String> {
        self.source.as_deref().map(source_name)
    }
}

/// 几种任务预计会有的步骤和权重（按步骤名里的字匹配）。
pub(crate) mod plan {
    /// 装原版或 Fabric。
    pub(crate) const VANILLA: &[(&str, f32)] =
        &[("游戏资源", 60.0), ("运行库", 15.0), ("客户端", 10.0)];
    /// 装 Forge 或 NeoForge：多了下载安装数据和跑 processor。
    pub(crate) const FORGE: &[(&str, f32)] = &[
        ("游戏资源", 45.0),
        ("处理器", 20.0),
        ("运行库", 10.0),
        ("依赖", 8.0),
        ("客户端", 7.0),
        ("安装数据", 5.0),
    ];
    /// 导入整合包。
    pub(crate) const MODPACK: &[(&str, f32)] = &[
        ("整合包文件", 40.0),
        ("游戏资源", 25.0),
        ("解压", 10.0),
        ("运行库", 10.0),
        ("客户端", 5.0),
    ];
    /// 下载 Mod、资源包、光影或更新它们。
    pub(crate) const CONTENT: &[(&str, f32)] = &[("Mod 文件", 85.0), ("前置|依赖", 10.0)];
    /// 下载 Java。
    pub(crate) const JAVA: &[(&str, f32)] = &[("下载 Java", 85.0), ("解压 Java", 10.0)];
    /// 导出整合包。
    pub(crate) const EXPORT: &[(&str, f32)] = &[
        ("读取文件", 30.0),
        ("Modrinth", 8.0),
        ("CurseForge", 7.0),
        ("打包", 55.0),
    ];
}

pub(crate) fn source_name(host: &str) -> String {
    let named = [
        ("bmclapi", "BMCLAPI 镜像"),
        ("mcimirror", "MCIM 镜像"),
        ("mojang.com", "Mojang 官方"),
        ("minecraft.net", "Mojang 官方"),
        ("neoforged.net", "NeoForge 官方"),
        ("fabricmc.net", "Fabric 官方"),
        ("minecraftforge.net", "Forge 官方"),
        ("modrinth.com", "Modrinth"),
        ("forgecdn.net", "CurseForge"),
        ("curseforge.com", "CurseForge"),
        ("github", "GitHub"),
    ];
    named
        .iter()
        .find(|(key, _)| host.contains(key))
        .map(|(_, name)| (*name).to_string())
        .unwrap_or_else(|| host.to_string())
}

/// 「12.4 MB/s」。
pub(crate) fn rate(bytes_per_sec: f64) -> String {
    if bytes_per_sec >= 1024.0 * 1024.0 {
        format!("{:.1} MB/s", bytes_per_sec / 1024.0 / 1024.0)
    } else {
        format!("{:.0} KB/s", bytes_per_sec / 1024.0)
    }
}

/// 「还要约 11 秒」「还要约 3 分钟」。
pub(crate) fn remaining(secs: f64) -> String {
    match secs {
        s if s < 1.5 => "马上就好".into(),
        s if s < 90.0 => format!("还要约 {:.0} 秒", s.ceil()),
        s => format!("还要约 {:.0} 分钟", (s / 60.0).ceil()),
    }
}

/// 「0.4 秒」「1 分 12 秒」。
pub(crate) fn took(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 10.0 {
        format!("{secs:.1} 秒")
    } else if secs < 60.0 {
        format!("{secs:.0} 秒")
    } else {
        format!("{} 分 {} 秒", d.as_secs() / 60, d.as_secs() % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(stage: &str, done: u64, total: u64, current: Option<&str>) -> Progress {
        Progress {
            stage: stage.into(),
            bytes_done: done,
            bytes_total: total,
            current: current.map(String::from),
            source: current.map(|_| "piston-data.mojang.com".into()),
            ..Default::default()
        }
    }

    #[test]
    fn records_steps_files_and_source() {
        let mut a = Activity::new("1.21.1", "解析版本");
        a.update(p("下载依赖库", 0, 100, Some("lwjgl.jar")));
        a.update(p("下载依赖库", 40, 100, Some("gson.jar")));
        a.update(p("下载依赖库", 50, 100, Some("lwjgl.jar")));
        assert_eq!(a.steps.len(), 2);
        assert!(a.steps[0].took.is_some());
        assert!(a.steps[1].took.is_none());
        // 同一个文件再报一次就挪到最前面，不重复。
        assert_eq!(a.recent, ["lwjgl.jar", "gson.jar"]);
        assert_eq!(a.source_label().as_deref(), Some("Mojang 官方"));
        a.update(p("下载游戏资源", 0, 10, None));
        assert_eq!(a.steps.len(), 3);
        assert_eq!(a.speed(), None);
    }

    #[test]
    fn concurrent_steps_add_up_and_never_go_backwards() {
        let plan: &[(&str, f32)] = &[("游戏资源", 60.0), ("运行库", 30.0), ("客户端", 10.0)];
        let mut a = Activity::new("1.21.1", "读取版本信息").expect(plan);
        assert_eq!(a.overall(), Some(0.0));
        // 客户端先下完：只占一成，不会一下到 100%。
        a.update(p("下载客户端", 10, 10, None));
        let after_client = a.overall().unwrap();
        assert!(after_client > 0.09 && after_client < 0.15, "{after_client}");
        // 资源和运行库同时下，两边交替报。
        a.update(p("下载游戏资源", 0, 300, Some("a.ogg")));
        a.update(p("下载运行库", 0, 100, Some("lwjgl.jar")));
        assert!(a.steps[0].took.is_some(), "只报了名字的第一步算做完");
        a.update(p("下载游戏资源", 150, 300, None));
        a.update(p("下载运行库", 50, 100, None));
        assert_eq!(a.bytes(), (210, 410));
        assert_eq!(a.active(), ["下载游戏资源", "下载运行库"]);
        let middle = a.overall().unwrap();
        assert!(middle > after_client && middle < 0.7, "{middle}");
        // 运行库下完：这一步打勾，资源还在下。
        a.update(p("下载运行库", 100, 100, None));
        assert!(
            a.steps
                .iter()
                .find(|s| s.name == "下载运行库")
                .unwrap()
                .took
                .is_some()
        );
        assert_eq!(a.active(), ["下载游戏资源"]);
        // 冒出来一个没列出的步骤：只占 1，总进度不往回退。
        let before = a.overall().unwrap();
        a.update(p("下载日志配置", 0, 1, None));
        assert!(a.overall().unwrap() >= before);
        a.update(p("下载游戏资源", 300, 300, None));
        a.update(p("下载日志配置", 1, 1, None));
        assert!(a.overall().unwrap() > 0.98);
        // 收尾的「安装完成」：就是 100%。
        a.update(Progress {
            stage: "安装完成".into(),
            files_done: 1,
            files_total: 1,
            bytes_done: 1,
            bytes_total: 1,
            ..Default::default()
        });
        assert_eq!(a.overall(), Some(1.0));
        assert!(a.active().is_empty());
    }

    #[test]
    fn a_finish_in_the_middle_is_not_the_end() {
        let plan: &[(&str, f32)] = &[("整合包文件", 50.0), ("游戏资源", 50.0)];
        let mut a = Activity::new("整合包", "准备导入整合包").expect(plan);
        a.update(p("下载游戏资源", 100, 100, None));
        a.update(Progress {
            stage: "安装完成".into(),
            files_done: 1,
            files_total: 1,
            ..Default::default()
        });
        a.update(p("下载整合包文件", 0, 100, None));
        let now = a.overall().unwrap();
        assert!(now < 0.6, "装完加载器后还有整合包文件：{now}");
    }

    #[test]
    fn names_sources_and_formats_numbers() {
        assert_eq!(source_name("bmclapi2.bangbang93.com"), "BMCLAPI 镜像");
        assert_eq!(source_name("cdn.modrinth.com"), "Modrinth");
        assert_eq!(source_name("mod.mcimirror.top"), "MCIM 镜像");
        assert_eq!(source_name("example.org"), "example.org");
        assert_eq!(rate(13.0 * 1024.0 * 1024.0), "13.0 MB/s");
        assert_eq!(rate(512.0 * 1024.0), "512 KB/s");
        assert_eq!(remaining(10.2), "还要约 11 秒");
        assert_eq!(remaining(200.0), "还要约 4 分钟");
        assert_eq!(took(Duration::from_millis(400)), "0.4 秒");
        assert_eq!(took(Duration::from_secs(72)), "1 分 12 秒");
    }
}
