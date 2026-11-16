# 狼吃羊棋：完整强解数据库

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](Cargo.toml)
[![Python](https://img.shields.io/badge/Python-3.10%2B-blue.svg)](scripts/recompute.ps1)
[![Release](https://img.shields.io/github/v/release/Cesium-developer/lang-chi-yang-qi)](https://github.com/Cesium-developer/lang-chi-yang-qi/releases/latest)

5×5 棋盘「狼吃羊棋」（Wolf Eats Sheep）的完整胜负表与精确距离表，含离线 GUI、求解源码和 CI 构建。

[English](README.md)

## 目录

- [特性](#特性)
- [求解结果](#求解结果)
- [规则](#规则)
- [环境要求](#环境要求)
- [安装](#安装)
- [使用](#使用)
- [仓库结构](#仓库结构)
- [测试与校验](#测试与校验)
- [复现](#复现)
- [引用](#引用)
- [许可](#许可)

## 特性

- 全状态空间覆盖：18,779,288,400 个合法局面，不只是从开局可达的局面。
- 每个局面都有精确结果（胜 / 和 / 负）。
- 每个非和棋局面都有精确 DTM（到强制终局的距离）。
- WDL 经全表扫描校验、零不一致；DTM 经 12 层逐层 Bellman 校验。
- 离线 GUI：任意局面分析、全部着法列表、可调手数上限、悔棋、棋盘翻转。
- 命令行单局面查询工具。
- 压缩数据库支持随机访问；归档文件不进 Git 历史。

## 求解结果

| 指标       | 数值                           |
| -------- | ---------------------------- |
| 已解局面数    | 18,779,288,400               |
| 覆盖范围     | 3 只狼、4–15 只羊、双方行棋方           |
| 初始局面 | **和棋**                       |
| WDL 校验   | 全局面零不一致                      |
| DTM 校验   | 12 层全部通过 Bellman 校验          |
| 最大 DTM   | 226 手                        |
| 数据库大小    | 4,824,928,613 字节（约 4.49 GiB） |

双方最优下，初始局面为和棋。这是**强解**：所有合法局面的结果都已确定。

基础表不设手数上限，重复局面判和。GUI 可在此基础上叠加可调手数上限，无需重算。

详见[结果说明](docs/results.md)、[完整报告](docs/%E5%BC%BA%E8%A7%A3%E7%BB%93%E6%9E%9C.md)和[校验日志](docs/verification/)。

## 规则

棋盘 5×5。开局上三行为 15 只羊，底行中间三格为 3 只狼，其余七格为空。

- 双方每步只能上下左右走到相邻空格。
- 羊不能吃子。
- 狼隔着**一个空格**跳吃羊，限上下左右方向。
- 羊少于 4 只时狼胜。
- 狼无合法走法时羊胜。

循环局面按和棋处理。原始中文规则见 [`docs/规则.txt`](docs/%E8%A7%84%E5%88%99.txt)。

## 环境要求

- GUI：现代浏览器 + 从 Release 解压的数据文件。
- 命令行查询：Python 3.10 或更高，仅用标准库。
- 复现：Windows、PowerShell、Python 3.10+，以及 GitHub Actions 产出的编译工具。本机不需要 Rust 工具链。

## 安装

本仓库只有源码，数据库通过 GitHub Release 发布。

1. 从[最新 Release](https://github.com/Cesium-developer/lang-chi-yang-qi/releases/latest)下载全部 12 个压缩包，对应羊数 `s=4` 到 `s=15`。

2. 同时下载 `SHA256SUMS.txt` 并校验。

3. 将 12 个包解压到同一目录，每包提供三个文件：
   
   | 文件                     | 内容            |
   | ---------------------- | ------------- |
   | `wdl_s{N}.wdlz`        | 压缩的 2-bit 胜负表 |
   | `dtm_exact_s{N}.dtmz`  | 非和棋局面的精确距离    |
   | `dtm_exact_s{N}.oridx` | 随机访问用的紧凑索引    |

4. 克隆源码：`git clone https://github.com/Cesium-developer/lang-chi-yang-qi.git`

归档文件刻意排除在 Git 历史之外，保证仓库轻量、克隆迅速。

## 使用

### GUI

打开 `solver_rs/启动强解GUI.cmd`（或直接打开 `solver_rs/gui.html`），选择解压后的数据目录。

GUI 完全离线，按需读取本地文件，最多缓存 128 页（8 MiB），不上传也不复制数据库。支持与已解引擎对弈、任意局面分析、列出全部着法及精确结果与距离、可调手数上限、悔棋、切换行棋方和棋盘翻转。

### 命令行

在仓库根目录运行：

```powershell
python solver_rs/wdlq.py
python solver_rs/wdlq.py 'SSSSS/SSSSS/SSSSS/...../.WWW.' wolf
```

棋盘记法：`S` 羊、`W` 狼、`.` 空格，行间用 `/` 分隔。第二个参数可选，指定行棋方（`wolf` 或 `sheep`）。

### 求解器程序

`Cargo.toml` 声明三个程序：`retro5`（WDL 求解器）、`retro_dtm_exact`（精确 DTM 求解器）、`verify`（WDL 全表校验器）。Windows 和 Linux 预编译版本作为每次 CI 的下载工件提供。

## 仓库结构

| 路径                                | 说明                       |
| --------------------------------- | ------------------------ |
| `solver_rs/retro5.rs`             | WDL 求解器：分层逆向分析，16 线程     |
| `solver_rs/retro_dtm_exact.rs`    | 精确 DTM 求解器，逐层 Bellman 校验 |
| `solver_rs/verify.rs`             | 独立的 WDL 全表校验器            |
| `solver_rs/gui.html`              | 离线交互 GUI                 |
| `solver_rs/wdlq.py`               | 命令行局面查询                  |
| `solver_rs/table_storage.py`      | 压缩、打包、存储校验               |
| `solver_rs/compress_dtm.py`       | DTM 压缩辅助                 |
| `scripts/recompute.ps1`           | Windows 端到端复现脚本          |
| `scripts/package_release_data.py` | Release 归档与 SHA-256 清单   |
| `.github/workflows/build.yml`     | Windows / Linux CI 构建    |

## 测试与校验

| 检查项                       | 结果      |
| ------------------------- | ------- |
| 全表扫描：所有局面及合法后继            | 零不一致    |
| 12 层 DTM Bellman 校验       | 全部通过    |
| WDL / DTMZ / ORIDX 压缩往返   | 逐字节一致   |
| Release ZIP CRC 与 SHA-256 | 打包脚本已校验 |

日志：[`verify_all.log`](docs/verification/verify_all.log)、[`retro_dtm_full.log`](docs/verification/retro_dtm_full.log)。

## 复现

编译工具由 CI 产出，本机无需安装 Rust。

1. 克隆仓库。
2. 打开 **Actions** 页，从一次成功的 **Build solver** 运行下载 `Windows compiled-tools` 工件。
3. 解压到 `build/solver`。
4. 确认 Python 3.10+ 在 `PATH` 中。
5. 在仓库根目录运行：

```powershell
pwsh -File scripts/recompute.ps1 -Threads <线程数>
```

脚本会重新生成 WDL、运行全表校验、计算 DTM 并逐层 Bellman 校验、压缩并验证数据，最后把 12 个归档和 `SHA256SUMS.txt` 写入配置的 Release 目录。脚本拒绝覆盖已有表文件或归档。

实测环境 Ryzen 7 9800X3D（16 逻辑线程、32 GB 内存）：WDL 求解 3,347.6 秒，WDL 校验 1,602.8 秒，DTM 求解 1,068.4 秒。这些是所附日志中的实测值，不代表其他硬件的表现。

`engine/` 下的 Python 模块实现了较早的 200 回合规则，仅作教学保留，不属于数据库的一部分。

## 许可

按 [MIT License](LICENSE) 发布。AI 辅助声明见 [AI_USAGE.md](AI_USAGE.md)。
