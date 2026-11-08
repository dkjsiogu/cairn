# 27 · 一个版本一个文件夹

> 0.42 起，每个版本是 `versions/<名字>/` 一个文件夹，里面有完整的版本 JSON、客户端 jar 和这个版本的全部游戏文件。版本之间没有依赖，删掉一个不影响别的。
>
> 以前的问题：
>
> - **三层继承**：整合包 → 加载器版本 → 原版。删掉下层，上层就启动不了，界面也不拦。
> - **游戏目录不统一**：原版和没装过 Mod 的版本共用 `.minecraft` 根目录，装过 Mod 的用 `versions/<id>/`，整合包在 `instances/<名字>/minecraft/`。
> - **删除不进回收站**：直接删掉，带着存档。
>
> 0.41 以前的数据不迁移（还没正式发布）。旧版本照样列出来，能用就用。

## 1. 磁盘布局

```text
.minecraft/
├─ versions/
│  └─ 生存 1.21.1/                 一个版本 = 一个文件夹
│     ├─ 生存 1.21.1.json          完整的版本 JSON：没有 inheritsFrom，带 "cairn" 记录
│     ├─ 生存 1.21.1.jar           原版客户端，从内容库硬链接
│     ├─ mods/ saves/ config/ resourcepacks/ shaderpacks/ screenshots/ options.txt …
│     └─ backups/                  恢复点和存档快照（以前就在游戏目录里）
├─ libraries/  assets/             共用，只是缓存，都能重新下载
├─ natives/<名字>/                  启动时解压的 native 库
└─ cache/cairn/profiles/           安装用的中间文件：原版和加载器的 profile，只是缓存
```

- **游戏目录就是版本文件夹**。没有「共用 .minecraft」的版本了。
- **`cache/cairn/profiles/`**：原版、Fabric、Forge、NeoForge 的 profile 先装在这里（和以前在 `versions/` 下的写法一样，带 `inheritsFrom`），再合并成完整的 JSON 写进版本文件夹。版本装好后不再需要它；留着是为了下次装同一个加载器时跳过下载。
- **其他启动器**：PCL、HMCL 和官方启动器能直接认出这些版本，因为 JSON 是完整的。

## 2. 版本 JSON 里的 `cairn` 记录

```json
"cairn": {
  "mc": "1.21.1",
  "loader": "neoforge",
  "loaderVersion": "21.1.209",
  "profile": "neoforge-21.1.209",
  "pack": { "name": "城市整合包", "format": "mrpack", "source": "city.mrpack" }
}
```

- `loader`：`vanilla`、`fabric`、`forge`、`neoforge`。
- `profile`：合并前用的是哪个 profile。
- `pack`：只有整合包有。
- 列表、补全、换加载器版本都读这里，不再从 JSON 内容猜。没有这条记录的（别的启动器装的）照旧猜。

## 3. 名字

- 安装时可以起名。默认名：
  - 原版：`1.21.1`
  - 加载器：`1.21.1 NeoForge`
  - 整合包：整合包自己的名字
- 重名时加数字：`1.21.1 NeoForge 2`。
- 不能用的字符：`/ \ : * ? " < > |` 和控制字符。前后不能有空格或点。最长 64 个字。可以用中文。
- **改名**：一起改文件夹、JSON、jar、JSON 里的 `id`，以及这个版本的启动参数（`launch-settings.json` 里按名字存）。游戏运行时不能改。
- natives 目录用名字的 ASCII 版本。名字里有中文时，后面加一段哈希，免得两个版本撞到同一个目录。

## 4. 安装、补全、换加载器

`core::versions::install(root, 名字, mc, 加载器, 加载器版本, …)`：

1. 在 `profiles/` 里装原版和加载器（下载、processor、复用本机已有的补丁客户端，都和以前一样）。
2. 合并继承链，写成 `versions/<名字>/<名字>.json`，加上 `cairn` 记录。
3. 把原版客户端 jar 硬链接到 `versions/<名字>/<名字>.jar`。不能硬链接时复制。

- **补全**：对同一个名字再跑一遍。游戏文件不动，JSON 和 jar 重写。
- **换加载器版本**：同一个名字，换一个加载器版本再跑一遍。之前留一份恢复点（JSON 和 jar）。
- **整合包**：加载器照上面装，整合包的文件解压进版本文件夹。HMCL、ATLauncher 这类自带版本 JSON 的整合包，把它的 JSON 当作一个 profile 放进 `profiles/`，再一样合并。

## 5. 删除

- 整个文件夹移到回收站，包括存档。删之前确认一次，确认时写清楚版本名和有几个世界。
- 移不到回收站时（比如数据目录在 `/tmp`、U 盘上，或者 Linux 上没有 `gio`），先说明原因，再问要不要直接删除；确认了才删，删了找不回来。
- 不检查依赖，因为没有依赖。
- 这个版本的 natives 目录和启动参数一起去掉。

## 6. 清理

设置里的「清理不用的文件」。先算能释放多少，点了才删：

| 删什么 | 怎么判断不用 |
| --- | --- |
| `profiles/` 里的 profile | 没有任何版本的 `cairn.profile` 用到它，也不在它的继承链上 |
| 资源文件 `assets/objects/` | 不在任何版本的资源索引里 |
| 资源索引 `assets/indexes/` | 没有版本用它 |
| 运行库 `libraries/` | 不在任何版本的库列表里 |

运行库有一个例外：`net/minecraft/`、`net/minecraftforge/`、`net/neoforged/`、`de/oceanlabs/` 下的不删。Forge 和 NeoForge 的 processor 会在这里生成补丁后的客户端，这些文件不写在版本 JSON 的库列表里，删了就启动不了。

清理时如果有安装或游戏在运行，就不清理。

## 7. 界面

- 版本页：去掉「显示基础版本」「被依赖」「被使用」。每一行就是一个版本文件夹。
- 详情：显示文件夹路径，加上「改名」。删除按钮写「移到回收站」。
- 安装页：多一个「名字」输入框，填好默认名。
- 设置：「清理不用的文件 · 可释放 X」。
- 内容、存档、截图页：游戏目录一律是版本文件夹，不再写 `.nailong-isolated`。

## 8. 命令行

```bash
cairn-cli install --name 生存 neoforge 1.21.1 21.1.209
cairn-cli list
cairn-cli rename 生存 家园
cairn-cli remove 家园            # 移到回收站；加 --permanent 直接删
cairn-cli clean                  # 只算能释放多少；--list 列出文件，--yes 执行
```

## 9. 测试

- **core**：名字校验、默认名、重名加数字（不分大小写）；整合包名字清洗；从 profile 认出加载器；合并后的 JSON 没有 `inheritsFrom` 和 `jar`、带 `cairn` 记录、jar 在文件夹里、删掉缓存照样能解析；补全保留游戏文件；同一个加载器两份互不影响，natives 目录不撞；改名连启动参数一起改；清理只删不用的，processor 生成的目录和在用的资源不动。
- **实装实启**（全新数据目录）：
  - 装了原版、两份 NeoForge 21.1.209（「生存」「测试」）和 Fabric 0.20.1。第二份 NeoForge 0 秒装好（直接用缓存）。客户端 jar 是硬链接，每个版本文件夹只占几十 KB。
  - 「生存」（NeoForge）和「1.21.1 Fabric」都启动到了主菜单，游戏文件写进各自的版本文件夹。
  - 导入一个整合包，名字里的冒号换成下划线，覆盖文件解压进版本文件夹，`cairn` 记录里带着整合包信息。
  - 改名、清理在硬链接复制的数据目录上跑过。清理列出的是 NeoForge 安装时 processor 用的工具库，启动用不到。
  - 在 `/tmp`（tmpfs）上删除：`gio` 拒绝放进回收站，报「这个位置不能用回收站」，不会直接删。
- **截图核对**：版本页（普通版本、整合包、旧数据）、改名、删除确认、安装页的名字、设置里的清理，深浅两种主题。
- **还没实测**：Windows 上的回收站和改名；Forge（老的 `jar` 字段写法）；真实数据上的清理效果。
