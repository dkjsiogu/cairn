# 26 · 自动更新

> 0.41 起，Windows 和 Linux 都能自动更新。
>
> - **Windows**：Cairn 在后台下载新版本，关掉 Cairn 时换上。
> - **Linux**：deb 装好后会加上 Cairn 的 apt 源，系统每天自动更新。也可以用 `sudo apt install cairn` 安装。

代码在 `core::update`（检查、下载、校验、替换）和 `dialogs/update.rs`（界面）。打包在 `scripts/build-deb.sh`、`scripts/build-apt-repo.sh` 和 `packaging/linux/apt/`。

## 1. 什么时候检查

- 打开 Cairn 时检查一次，之后每 12 小时一次。
- 只查 GitHub 上标为 Latest 的正式版。
- 设置右上角可以手动检查。

## 2. 按装法分

| 装法 | 怎么认出来 | 自动更新 | 手动更新 |
| --- | --- | --- | --- |
| Windows exe | Windows | 后台下载，关掉时换上 | 「下载并更新」→「现在重启」 |
| deb + apt 源 | 程序是 `/usr/bin/cairn`，dpkg 里有 cairn，apt 源里有 Cairn 的地址 | 系统每天更新 | 「更新」→ 系统授权窗口 →「现在重启」 |
| deb，没有 apt 源 | 同上，但没有 apt 源 | 无 | 同上 |
| 别的 | 自己编译的、直接运行二进制的 | 无 | 下载 deb，给出安装命令 |

## 3. Windows

1. 发现新版本，并且「自动更新」开着（默认开），就在后台下载 `cairn.exe`。
2. 下载完按发布里的 `SHA256SUMS` 校验，然后记在缓存目录的 `update/staged.json`：版本、文件、SHA-256。
3. 侧栏提示变成「重启更新到 X」。
4. 关掉 Cairn 时换上新版本，下次打开就是新版本：
   - 先把正在运行的 `cairn.exe` 改名成 `cairn.old.exe`（Windows 允许给运行中的程序改名），再把新文件放到原位置。
   - 放不进去就把旧的改回来。
5. 换上前再校验一次。对不上就删掉，不换。
6. 被强制结束、没来得及换的，下次打开时先换上，再打开新版本。
7. 下次启动时删掉 `cairn.old.exe`。

另外几种情况：

- 程序所在的文件夹不能写（比如放在 `Program Files`）：不自动下载，弹窗里提示到 GitHub 下载。
- 跳过的版本不自动下载。
- 设置里可以关掉「自动更新」。关掉后只提示新版本。

## 4. Linux

### 4.1 apt 源

- 地址：`https://dkjsiogu.github.io/cairn/apt`，托管在 GitHub Pages。
- 内容：最近 3 个正式版的 deb。留 3 个，是因为本地索引旧了的机器还能下到它记着的那个版本。
- 签名：专用的 ed25519 密钥，指纹 `EA8E4CE07C4F56C99A4ABAC59CB5F48B25C9C5C4`。
  - 公钥在 `packaging/linux/apt/cairn-archive-keyring.gpg`，deb 把它装到 `/usr/share/keyrings/`。
  - 私钥存在仓库 Secrets 的 `APT_SIGNING_KEY`，本机备份在 `~/.local/share/cairn-release/`。
  - 建源脚本最后用这把公钥验签。签错了，发版就失败。
- Release 文件：`Origin: Cairn`、`Label: Cairn`、`Suite: stable`。
- 发版：`release.yml` 的 `apt` 任务。发完 GitHub Release 后，下载最近 3 个版本的 deb，建源，签名，部署到 Pages。
- Pages 首页（`packaging/linux/apt/index.html`）写着安装命令。

### 4.2 deb 自带源

参考 Chrome 的 deb：装 Cairn 时顺便加上 apt 源，以后跟着系统更新。

`postinst` 写两个文件：

| 文件 | 内容 |
| --- | --- |
| `/etc/apt/sources.list.d/cairn.sources` | apt 源（deb822 格式），`Signed-By` 指向装好的公钥 |
| `/etc/apt/apt.conf.d/52cairn-auto-update` | `Unattended-Upgrade::Origins-Pattern:: "origin=Cairn,label=Cairn";` |

- 只在第一次安装，或从 0.41.0 以前的版本升级时写。玩家删掉其中一个以后，升级不会再加回来。
- 玩家已经自己加过同一个地址（`.list` 也算），就不再写源，免得重复。
- 卸载（remove）时保留这两个文件；彻底删除（purge）时删掉。
- 这两个文件不是 conffile。升级时不会弹出「是否覆盖配置文件」。

### 4.3 系统每天更新

- Ubuntu 默认开着 unattended-upgrades，每天运行一次。上面那行配置把 Cairn 加进允许自动升级的来源。
- 程序被换掉以后，正在运行的 Cairn 不受影响。启动器每分钟看一次 `/proc/self/exe`：末尾多了 ` (deleted)`，就说明被换掉了。这时用 `dpkg-query` 读出新版本，侧栏提示「重启更新到 X」。
- 系统没装或没开 unattended-upgrades（比如部分 Debian）时，用 `sudo apt upgrade` 或系统的软件更新器也能收到新版本。

### 4.4 在启动器里马上更新

1. 下载 deb，按 `SHA256SUMS` 校验。
2. 用 `pkexec /usr/bin/apt-get install -y <deb>` 弹出系统的授权窗口。
3. 装好后提示「重启 Cairn 后生效」，点「现在重启」。
4. 装不上时，显示原因，并给出能复制的命令：
   - 取消授权：「没有授权」。
   - 没有授权窗口（比如没有 polkit 代理）：「没能弹出授权窗口」。
   - apt 被占用：「系统正在安装别的软件，稍后再试」。

游戏运行时，「现在重启」不能点。

## 5. 界面

- **侧栏提示**：有新版本时显示「新版本 X」，新版本就绪时显示「重启更新到 X」。窄侧栏只显示「新」，悬停时写清楚。
- **更新弹窗**：更新说明、进度或状态、操作按钮。
  - 状态一行写清楚现在在哪一步：下载进度、「在系统弹出的窗口里输入密码」、「已下载。关掉 Cairn 时自动换成新版本。」或「已装好。重启 Cairn 后生效。」
  - Linux 系统在后台装好、还没查到更新说明时，写「更新说明在 GitHub 上」。
- **设置**：外观卡片最后一行是「自动更新」。
  - Windows：开关。
  - Linux：显示「由系统每天更新」或「没有 apt 源，需手动更新」。

## 6. 测试

- **core**：认出 apt 源（deb822、一行式、注释掉的不算）；apt 报错的说明；暂存的版本只在比现在新、文件还在时才算。
- **界面**：「跳过」不影响已就绪的版本。
- **postinst**：用临时目录代替 `/etc` 跑了这几种情况：第一次安装、从 0.40.1 升级、删掉后再升级、玩家已加过源、remove、purge。
- **unattended-upgrades**：用它自己的 `match_whitelist_string` 核对，Cairn 的来源能匹配，Ubuntu 的不能。
- **apt 源**：本地用真密钥建源，起一个 HTTP 服务，用独立的 apt 目录跑 `apt-get update` 和 `apt-get install --download-only cairn`：验签通过，选中最新版本。
- **截图核对**：更新弹窗的各个状态、侧栏提示、设置里的「自动更新」，深浅两种主题。
- **还没实测**：Windows 上关掉时替换；真机上 unattended-upgrades 跑一次；pkexec 授权窗口。
