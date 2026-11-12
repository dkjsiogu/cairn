# 28 · 导出整合包

> 0.43 起，任何一个版本都能导出成 `.mrpack` 整合包。不是把版本文件夹整个打包，而是导出一个最小的包：
>
> - `mods/`、`resourcepacks/`、`shaderpacks/` 里能在 Modrinth 或 CurseForge 下到的文件，只记下载地址和哈希。安装的人自己去下。
> - 网上找不到的文件，连同 config、kubejs、脚本、世界存档这些，打包进去。
>
> 一个 150 个 Mod 的整合包：Mod 共 901 MB，导出来 307 MB，大部分是没公开的资源包和配置。

代码在 `core::export`（列出内容、查找、写包）和 `dialogs/export.rs`（界面）。命令行是 `cairn-cli export`。

## 1. 格式

`.mrpack` 是 Modrinth 的整合包格式。Cairn、Prism、HMCL、PCL 都能导入。

```text
新模拟大都市-1.0.0.mrpack
├─ modrinth.index.json      清单：MC 和加载器版本、要下载的文件
└─ overrides/               原样放进游戏目录的文件
   ├─ config/  kubejs/  defaultconfigs/  options.txt …
   ├─ mods/自己做的.jar        网上找不到的 Mod
   └─ saves/我的世界/          勾选了的世界
```

清单里每个要下载的文件：

```json
{
  "path": "mods/sodium-neoforge-0.6.0.jar",
  "hashes": { "sha1": "…", "sha512": "…" },
  "env": { "client": "required", "server": "required" },
  "downloads": ["https://cdn.modrinth.com/data/…/sodium-neoforge-0.6.0.jar"],
  "fileSize": 1048576
}
```

`dependencies` 写 `minecraft` 和加载器（`fabric-loader`、`forge`、`neoforge`），从版本 JSON 的 `cairn` 记录读；别的启动器装的版本按 JSON 内容猜。

## 2. 哪些文件去网上找

- 只找 `mods/`、`resourcepacks/`、`shaderpacks/` 第一层的 `.jar` 和 `.zip`。子文件夹里的、别的扩展名的一律打包。
- `mods/*.disabled`（关掉的 Mod）不导出。
- 先按 SHA-1 问 Modrinth（`POST /v2/version_files`），连不上就问 MCIM 镜像。
- Modrinth 没有的，按指纹问 CurseForge（`POST /v1/fingerprints/432`）。指纹是去掉空白字节（9、10、13、32）后的 MurmurHash2，种子 1。
  - 有 API key 时问官方；没有或连不上时问 MCIM 镜像。**API key 不发给镜像。**
  - 作者关掉了第三方下载的文件，CurseForge 不给地址，这些打包进去。
- 哈希对得上才算找到。改过的 jar 哈希不一样，自然会打包。
- 两边都找不到的，导出后列在「这些文件网上没有，已打包」里。

CurseForge 的地址（`edge.forgecdn.net`）不在 Modrinth 规范允许的域名里。Cairn 照装；别的启动器可能会提示，要你确认。

## 3. 哪些文件不导出

从来不导出：

- 日志、崩溃报告、截图、缓存：`logs/`、`crash-reports/`、`screenshots/`、`.cache/`、`*.log`、`hs_err_pid*`、名字带 cache 的。
- 备份：`backups/`。
- 启动器自己的文件：`accounts.json`、`launch-settings.json`、`hmclversion.cfg`、`PCL/`、`usercache.json` 等。
- 版本文件本身：`<版本名>.json`、`<版本名>.jar`、`natives/`。
- 点开头的文件和文件夹（`.fabric/`、`.mixin.out/` 这类）。

没开版本隔离的版本，游戏目录就是 `.minecraft`。这时 `versions/`、`libraries/`、`assets/` 也在这一层，一样不导出。

## 4. 默认勾选

导出前列出游戏目录里的每个文件夹和文件，可以勾选。

- 默认勾上：`mods/`、`config/`、`kubejs/`、`resourcepacks/`、`shaderpacks/`、`options.txt` 这些。
- 默认不勾：
  - 世界存档。每个世界单独列出来，自己选要带哪个。
  - 小地图和录像的数据：`journeymap/`、`xaero*`、`replay_recordings/`、`schematics/`、`distant_horizons*`。
  - `servers.dat`（服务器列表）。
  - `local/`（KubeJS 放这台电脑自己设置的地方）。

## 5. 界面

- 版本详情：「导出整合包」按钮。版本列表右键也有。
- 弹窗：
  - 名字（默认版本名）、版本号（默认 1.0.0）。
  - 文件列表：每行一个勾选框、名字、大小。世界单独一段。「全选」「恢复默认」。
  - 「导出…」选保存位置，然后在任务卡片里显示进度：读取文件 → 在 Modrinth 上查找 → 在 CurseForge 上查找 → 打包。
- 导出完：多少个文件只记地址、多少个打包了、包多大；路径和「在文件夹中显示」；网上找不到的文件列表；要提醒的事。
- 提醒：用了整合包自带组件补丁的版本（比如 Prism 整合包换了 LWJGL），`.mrpack` 只记得住 MC 和加载器版本，装出来可能不一样。
- 导出时不能启动这个版本，也不能改名、删除。

## 6. 命令行

```bash
cairn-cli export 生存 ~/生存.mrpack                       # 默认勾选的内容
cairn-cli export 生存 ~/生存.mrpack --version 1.2.0
cairn-cli export 生存 ~/生存.mrpack --include mods --include config --include "saves/我的世界"
```

## 7. 测试

- **core**：CurseForge 指纹和参考实现一致；列表会跳过日志、缓存、版本文件，世界单独列出、默认不勾；只查第一层的 jar/zip；网上找得到的记地址、找不到的打包（假的查找函数）。
- **真实导出**：一个 NeoForge 1.21.1 整合包，150 个文件只记地址（125 个 Modrinth、25 个 CurseForge），4 个 Mod 和 4 个资源包打包，307 MB，debug 构建 64 秒。
- **再导入**：用 Cairn 把导出的包装到一个空的数据目录。150 个 Mod 的 SHA-1 和原来一样（3 个关掉的 Mod 不导出），资源包一样，70 秒启动到主菜单。
- **截图核对**：导出弹窗、导出完成，深浅两种主题。
