# 狼吃羊棋：完整强解数据库

仓库名：lang-chi-yang-qi  
英文名：Lang Chi Yang Qi / Wolf Eats Sheep

## 结果

- 覆盖 18,779,288,400 个局面：3 只狼、4–15 只羊、双方行棋方都计入；不是只算从开局可达的局面。
- 开局在双方最优走法下为和棋。
- 完整 WDL（胜、和、负）校验扫描全部局面及合法后继，零不一致。
- 所有非和棋局面都有精确 DTM（到强制终局的距离）；12 个羊数层全部通过 Bellman 校验。当前最大距离为 226 手。
- WDL、DTM 与随机查询索引合计 4,824,928,613 字节，约 4.49 GiB。
- 基础数据库不设手数上限，重复局面按和棋。GUI 另提供可调限手判和功能。

最终结果和资源记录见 docs/强解结果.md；完整校验输出见 docs/verification。求解过程文档保留了研究历史，其中旧数字已经明确标记为过时。

## 规则范围

开局为上三行 15 只羊、底行中间三个格 3 只狼，其余七格为空。双方每步都只能上下左右走到相邻空格；羊不能吃子，狼可以隔一个空格跳吃羊。羊少于 4 只时狼胜；狼无合法走法时羊胜。

## 下载和使用数据库

GitHub Release 按羊数 s=4 至 s=15 提供 12 个压缩包。每个包包含该层的 WDLZ 胜负表、DTMZ 精确距离表和 ORIDX 索引。下载全部 12 包即可得到完整库；用 Release 里的 SHA256SUMS.txt 检查完整性。

打开 solver_rs/启动强解GUI.cmd，选择解压后的数据目录即可离线研究。界面按需读取本地数据，不会上传文件。

## 从源码重算

编译由 GitHub Actions 完成，不需要在本机安装 Rust。Actions 的 Build solver 工作流会为 Windows 和 Linux 编译最终求解器，并提供可下载的工具包。

Windows 重算步骤：

1. 克隆仓库。
2. 在 Actions 页面下载一次成功的 Windows compiled-tools 工件，并解压到仓库的 build/solver。
3. 安装 Python 3.10 或更高版本。
4. 在 PowerShell 运行：

    pwsh -File scripts/recompute.ps1 -Threads 16

脚本会从头生成 WDL，运行全表校验，求精确 DTM 并逐层做 Bellman 校验，压缩和验证数据，最后生成 12 个 Release 包及 SHA256SUMS.txt。脚本只清理本次生成且校验过的原始 WDL 中间文件。需要较多 CPU、内存和磁盘空间；上限保护和实测资源记录见报告。

最终求解源码在 solver_rs。Rust 程序使用标准库；Python 数据处理程序只使用标准库。Cargo.toml 列出最终的 WDL 求解器、DTM 求解器和 WDL 全表校验器。

## GitHub 标签与许可

建议的 GitHub topics：wolf-eats-sheep、lang-chi-yang-qi、tablebase、retrograde-analysis、game-solving、perfect-information-game、board-game、game-theory、endgame-tablebase、rust。

GitHub topic 仅支持小写英文字母、数字和连字符；中文“狼吃羊棋”会出现在仓库描述与中英文 README 中。

源码、文档和表数据按 MIT License 发布。AI 使用声明见 AI_USAGE.md。
