# Fuck My Shit Mountain — 仓库内维护副本

来源：[Fuck_My_Shit_Mountain](https://github.com/XiNian-dada/Fuck_My_Shit_Mountain)。从本机同名技能完整复制，保留 prompts、rubrics、templates、examples 和打包工具；当前副本由 Clipboard 仓库独立维护。

入口为 [SKILL.md](SKILL.md)，Clipboard 专用规则见 [references/clipboard.md](references/clipboard.md)。支持 26 个专项维度、全量与增量审计，以及 Markdown、HTML、JSON 和对话输出。此目录的更新不会同步到个人全局技能。

使用示例：

```text
读取 skills/fuck-my-shit-mountain/SKILL.md，全量审计当前仓库，输出中文 Markdown。

使用仓库内 fuck-my-shit-mountain，对 main...HEAD 做安全与数据完整性增量审计，输出 JSON。

使用仓库内 fuck-my-shit-mountain，审查当前未提交变更，在对话中给出结果。
```

语言默认跟随对话；一般审计默认全量，具体关注点自动映射专项维度；报告请求默认 Markdown，对话审查默认 stdout。只有影响范围判断的歧义才需要澄清。审计本身不会修改应用代码或触发发布。

所有审计文件统一保存到本技能的 `result/`（仓库路径：`skills/fuck-my-shit-mountain/result/`），包括 Markdown、HTML、JSON 报告、历史元数据、修复计划及证据附件，不写入项目根目录。可按审计批次建立子目录，重名时保留旧报告。该目录中的结果保持本地存储，不进入 Git 或技能分发 ZIP；`stdout` 仍只在对话中输出。

本地检查：

```powershell
python -X utf8 -B -m unittest discover -s skills/fuck-my-shit-mountain/tests -v
python -X utf8 -B skills/fuck-my-shit-mountain/scripts/project_inventory.py . --format text --language zh
python -X utf8 -B skills/fuck-my-shit-mountain/scripts/report_lint.py --modes security skills/fuck-my-shit-mountain/result/audit-report.json
python -X utf8 -B skills/fuck-my-shit-mountain/scripts/package_skill.py --dry-run
```

检查范围与限制见 [报告格式规则](references/report-format.md)。测试使用临时目录和合成数据，不读取真实剪贴板数据。
