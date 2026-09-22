# Clipboard Desktop v1.7.0

> 崩溃安全原子写入、同步可靠性强化、快捷键深度定制与一轮稳定性修复
>
> Released: 2026-09-22

---

## 新功能

### 悬浮面板与快捷键

- **悬浮面板点击动作** — 设置可配置点击行为 | [`1c668a5`](https://github.com/muutot/Clipboard/commit/1c668a5df57f40ae4846dd65b7b41d715eaab65c)
- **主窗自定义绑定生效** — 条目操作与快速复制绑定在主窗口内同样生效 | [`e2e3533`](https://github.com/muutot/Clipboard/commit/e2e3533422d266eff5f9a46893bd3b966fccebd3)
- **悬浮粘贴目标记忆** — 切换悬浮面板时记住快速粘贴目标，粘贴跟踪前台窗口 | [`1ab1905`](https://github.com/muutot/Clipboard/commit/1ab19054d341bdc48afc274c8490d97af2ebdef6) [`6da9ab6`](https://github.com/muutot/Clipboard/commit/6da9ab621baf5e8df725dbeb81051e40322b5c0f)

### 搜索与使用统计

- **真实匹配总数与截断提示** — 搜索页报告真实匹配总数与是否截断 | [`d34d4af`](https://github.com/muutot/Clipboard/commit/d34d4afe1a818a4ef21282629c70fcb872fc694c)
- **重拷冻结采集时间** — 重拷不刷新采集时间，并盖戳使用记录，搜索缓存随戳失效 | [`1f51e0a`](https://github.com/muutot/Clipboard/commit/1f51e0a5638235e98a48ab3bc4f8aef65d9c7d7d) [`830c4dc`](https://github.com/muutot/Clipboard/commit/830c4dcd50d95ca68e32a9e9c9805ab202d4818c)

### 安全

- **本地 API 资源边界** — 限制请求读取字节数与并发连接槽 | [`b0a6638`](https://github.com/muutot/Clipboard/commit/b0a66380e3fc21b8f2771d6848a1d27117ccdb40)
- **同步密钥脱敏** — `SyncSettings` 的 Debug 输出不再泄露密钥 | [`56b2ded`](https://github.com/muutot/Clipboard/commit/56b2deda4a0002a34bbb7c0b0bf73181e745ad55)
- **替换图标源校验** — 检查 ICO magic 与 SVG prolog，拒绝伪造源 | [`11c6505`](https://github.com/muutot/Clipboard/commit/11c65050737b48e1f7b0685b9671de851fdc8039)

---

## 崩溃安全与原子写入

- **文件与截图落盘** — 经 staging + fsync + 原子 rename，崩溃不再留下半截文件 | [`e650a8f`](https://github.com/muutot/Clipboard/commit/e650a8f52690f5d436e3e39e80fca5af8947218b) [`a03faf2`](https://github.com/muutot/Clipboard/commit/a03faf25d5579a020148d0159692cea453bfbb9e) [`d70e642`](https://github.com/muutot/Clipboard/commit/d70e6427ba93ae359bb2309a14363e123716e46c)
- **剪贴板图片原子采集** — 图片经原子写入，带大小门控跳过；RGBA 重编码为 PNG | [`b39ec15`](https://github.com/muutot/Clipboard/commit/b39ec1551b260978741529ad5adafb63f8e37881) [`314f4dc`](https://github.com/muutot/Clipboard/commit/314f4dc6ab29f8f9a4834a88c43df07a9c701935)
- **应用图标原子写入** — 图标经 atomic writer 落盘，拒绝失败的 GDI 捕获并写原子 PNG | [`d11e157`](https://github.com/muutot/Clipboard/commit/d11e1570c97314d89b34a894e2421d515d8aec02) [`eeb2e36`](https://github.com/muutot/Clipboard/commit/eeb2e3620f3f63232cf7d64207cd50686f1f06f5)
- **导入与去重** — 导入按暂存字节哈希关闭竞态，重拷/另存为生成唯一内容哈希 | [`99a6552`](https://github.com/muutot/Clipboard/commit/99a6552863489b3d48cfa07a13ed85457965e0f7) [`5c60e77`](https://github.com/muutot/Clipboard/commit/5c60e77a9ec23a5afca597bc9ce3866ce362145d)
- **搜索 manifest** — 容忍瞬时读取失败并原子写入，路径处理经 `Path` 保持 clippy 清洁 | [`aeb317c`](https://github.com/muutot/Clipboard/commit/aeb317cb01f22d9d89f3613119643dad63fc6586) [`3bbddc5`](https://github.com/muutot/Clipboard/commit/3bbddc53bec3558cfb777791c496a91b53465ee3)

---

## 同步可靠性

- **孤儿段接纳** — 上传中断不再拖垮对端拉取 | [`abc7e36`](https://github.com/muutot/Clipboard/commit/abc7e3632b5dd5b95aa133f757cb45d78cbdc024)
- **检查点游标与错误** — 游标领先于冻结检查点向量，恢复时保留原始拉取错误 | [`251c7f7`](https://github.com/muutot/Clipboard/commit/251c7f774ef130f5aea50d604f1a4dc85f0a8161) [`2298773`](https://github.com/muutot/Clipboard/commit/2298773fc75bdd0efb74068b508290aa8d0e31ba)
- **锁与线程池** — `sync_now` 走阻塞线程池，手动同步锁竞争后尽快重试自动同步 | [`e553cd3`](https://github.com/muutot/Clipboard/commit/e553cd3c214768f68d8fd9ff8c5a3df2c5ebd482) [`16dcc33`](https://github.com/muutot/Clipboard/commit/16dcc33dd1deed509ea9bf065153242cc42363c4)
- **对象存储兼容** — 压实阶段容忍缺失 ETag 的对象存储 | [`6a83bc2`](https://github.com/muutot/Clipboard/commit/6a83bc2afce291ad0f954df7b360a2371091777e)

---

## 剪贴板采集与粘贴

- **Windows 剪贴板加固** — 先分配缓冲区再 EmptyClipboard，OpenClipboard 失败重试而非丢弃采集 | [`c64e5a6`](https://github.com/muutot/Clipboard/commit/c64e5a6c7738fbb1ac63c38ace5e591347e564f3) [`8af95c5`](https://github.com/muutot/Clipboard/commit/8af95c5e94840cca7f6d768557e84541e1e80941)
- **DIB 解码** — 校验 biCompression、尊重 BITFIELDS 掩码布局、顶行 DIB 不再垂直翻转 | [`245b93c`](https://github.com/muutot/Clipboard/commit/245b93c3219989ee8139567b168a9dbf7f172cb7) [`abe8289`](https://github.com/muutot/Clipboard/commit/abe8289d0a828a71136b5de8d1325cfdf3aba975)
- **自触发标记** — 文件复制注册自触发，写入失败清除标记，托盘/API 复制失败时取消标记 | [`01ad0b9`](https://github.com/muutot/Clipboard/commit/01ad0b9cea47221987a41c20ebd734ed5e343d95) [`51f922f`](https://github.com/muutot/Clipboard/commit/51f922fe5866c43ff2335fadede92fa9eeef546b) [`a3cca40`](https://github.com/muutot/Clipboard/commit/a3cca405d9cce84cf7f012ddf48d59ee10969eab)
- **文件复制与粘贴** — HDROP 落地后文本负载为尽力而为，文件粘贴写为 OS file drop | [`85639c2`](https://github.com/muutot/Clipboard/commit/85639c2a0599cd8e70b54afa92c39eccaa560d5e) [`23f7046`](https://github.com/muutot/Clipboard/commit/23f7046545aeeb4fa2e65cf8feebeb3f8f108e6f)
- **前景应用与大图** — 读剪贴板后复查前景应用，大图放宽媒体自触发窗口 | [`65e3daf`](https://github.com/muutot/Clipboard/commit/65e3daffeb8aeb5ccdf7dcac0dac1e2a446c7c2e) [`09062fa`](https://github.com/muutot/Clipboard/commit/09062fab11968e6775f053a5628094aa1b973b4b)
- **重拷与缩略图** — 重拷提升 last-used 时间戳，重命名保留生成的 thumbnail `preview_path` | [`b0dfd1e`](https://github.com/muutot/Clipboard/commit/b0dfd1e647f1fb72477fdc012261bb0d63d3e5f2) [`13d6e66`](https://github.com/muutot/Clipboard/commit/13d6e662d3b27e953ba7636549b409f662d82c49)
- **macOS 采集** — 镜像捕获使用唯一临时名 | [`54f1832`](https://github.com/muutot/Clipboard/commit/54f1832e420ab6d78f5d11fd34265a3c7040c23b)

---

## 快捷键与输入

- **IME 合成期忽略 keydown** — 输入法合成中 Escape 不再隐藏窗口 | [`424c7a9`](https://github.com/muutot/Clipboard/commit/424c7a961c487c9c045351c745fbd3e78ef80ef0)
- **可编辑宿主内按键** — Enter/Space 留在 contenteditable 内，Ctrl+A 透传有测试钉住 | [`13defb4`](https://github.com/muutot/Clipboard/commit/13defb46190a90e1e72418cbf5698b6275999400) [`4e673c6`](https://github.com/muutot/Clipboard/commit/4e673c6a09a6623fddbbdb6f03f3c30cae7057fe)
- **自定义绑定全面生效** — Enter 走 copyItem 绑定语义，Space 尊重 openDetail 停用态，Backspace 清除选择可重绑，hideWindow 可重绑/停用 | [`0441380`](https://github.com/muutot/Clipboard/commit/04413808311b51cad6388a382cfbef53e4fcdbef) [`ac8e1ad`](https://github.com/muutot/Clipboard/commit/ac8e1ad1f1c601404ccae1e42b9e651742c5a524) [`4edd762`](https://github.com/muutot/Clipboard/commit/4edd762593ec9f560f6b5031fa6be947b48fed58) [`34fb382`](https://github.com/muutot/Clipboard/commit/34fb382c3a49274445e3121489d66a84b08e8776)
- **Alt 组合与 Space 穿透** — Alt 修饰的聚焦搜索和弦可穿透可编辑目标，无选中时 Space 下落 | [`f892f7e`](https://github.com/muutot/Clipboard/commit/f892f7e46dd24238bc18b48be4404f20c9152424) [`61f7fa0`](https://github.com/muutot/Clipboard/commit/61f7fa018c78b3f30ffd5ccce4385548bc8ce21d)
- **双击热键回绕** — GetTickCount 回绕后双击检测仍有效 | [`abe238d`](https://github.com/muutot/Clipboard/commit/abe238d80730856664b383043240511d66bb44b9)
- **快捷配置损坏自愈** — 隔离重命名失败时移除损坏配置 | [`e6fb540`](https://github.com/muutot/Clipboard/commit/e6fb540221c792a5e43f8643e3ca14585f71e9e3)
- **键盘选中可见** — 虚拟窗口滚动保持键盘选中可见 | [`70d197c`](https://github.com/muutot/Clipboard/commit/70d197c93e8d04550d223e8468f0f0d45d800b79)

---

## 搜索、历史与快捷动作

- **日期区间** — 昨天/上周按日历算术处理 DST，上月区间正确落在月末，非法日期候选后继续扫描 | [`9e052a2`](https://github.com/muutot/Clipboard/commit/9e052a27961672cc4da78e0aa7f5bb33fb18cecf) [`6d364d5`](https://github.com/muutot/Clipboard/commit/6d364d509d3b3ebf068c3a00d89c6ba6c256d3db) [`9dd885d`](https://github.com/muutot/Clipboard/commit/9dd885d37b18ecdae23f780e291dba9703ddb1d0) [`c34af02`](https://github.com/muutot/Clipboard/commit/c34af02ed991f1fe9cbd922005802e5c8718294a)
- **排序稳定** — 平局排序键保持输入相关性顺序，分页序加唯一 id 决胜 | [`0311301`](https://github.com/muutot/Clipboard/commit/031130162d00ea77863b2be720da0b24d2ca8c24) [`85acdda`](https://github.com/muutot/Clipboard/commit/85acdda22c477b4f84bf3e437aeb5b321d247035)
- **快捷动作检测** — 统一 HEX 颜色检测（含 8 位），URL 在 CJK 标点处截断并剥离句尾符号 | [`36c4b91`](https://github.com/muutot/Clipboard/commit/36c4b91607aef05882f3ce1730acfc2f97a6763b) [`a8f575c`](https://github.com/muutot/Clipboard/commit/a8f575c7739876d255bce77a1026aef13a877c46) [`aa9b527`](https://github.com/muutot/Clipboard/commit/aa9b527fb358f0d8ca7a4bd48b5f7e306614c411)

---

## 存储与清理

- **回收站与收藏** — 拒绝回收站行设收藏，关闭单删路径的收藏守卫竞态，过期清理保护已收藏回收站记录 | [`b0f9eba`](https://github.com/muutot/Clipboard/commit/b0f9ebaf58edeaf332db5a64011d5157a8cce2f8) [`d3fd029`](https://github.com/muutot/Clipboard/commit/d3fd029d11c058ff499fb0a918289c2e37ef558a) [`a9d127a`](https://github.com/muutot/Clipboard/commit/a9d127ab407fe3520007eba376fa01bd3631dc21)
- **导入去重** — 重复记录跳过而非重写，未知 PPaste 记录计入跳过 | [`7f30726`](https://github.com/muutot/Clipboard/commit/7f3072630c25c562e991d4aa7d361dab292db372) [`b567dfe`](https://github.com/muutot/Clipboard/commit/b567dfec13f793e388fdac363dbdda33fe05940e)
- **批量操作** — 批量删除失败回滚后重同步历史，一次性剪除非法批量选择 | [`4587262`](https://github.com/muutot/Clipboard/commit/4587262d57d9213fd7ded8d109940be0aacf670e) [`335b400`](https://github.com/muutot/Clipboard/commit/335b400b94fd14f5c5e7346abf8500dcab8aea46)
- **修复与校验** — 丢弃 repair 中已忽略的 quick_check | [`d9a84e2`](https://github.com/muutot/Clipboard/commit/d9a84e27ba0a5f53c1d2e45171648fcec2206b8b)
- **测试** — 收藏拒绝后重建 favorited-bin fixture | [`34b3045`](https://github.com/muutot/Clipboard/commit/34b30458a681215ede4fee1ec2d215879eb20c01)

---

## 设置与界面

- **设置持久化** — 关闭时刷写挂起写入并收敛远端更新，远端可清除 activePresetId，部分负载保留当前预设 | [`772313c`](https://github.com/muutot/Clipboard/commit/772313c61cfb0a331900f5132d34ccc40c3bc247) [`d445d6a`](https://github.com/muutot/Clipboard/commit/d445d6adfb3c3babb565011306ba3464f1dfd740) [`6e0304c`](https://github.com/muutot/Clipboard/commit/6e0304cd8a5cabf17f0580f003b6422e0042fa04)
- **数值钳制** — 同步与历史上限输入钳制，输入值不再清空保留策略 | [`ddc4f65`](https://github.com/muutot/Clipboard/commit/ddc4f654fa1f0721c63576dee2d1fd2be759f261) [`569af76`](https://github.com/muutot/Clipboard/commit/569af76c29de436c112584abaa37bb9da4ea25aa)
- **多语言** — 占位符全量替换、`$` 模式忽略，设置搜索文案走 locale key，外观 eyebrow 文案可解析 | [`2a73383`](https://github.com/muutot/Clipboard/commit/2a733838bb219409698e6e0cc4d734d443e2e0aa) [`c506778`](https://github.com/muutot/Clipboard/commit/c5067780a9a1dff22c9713937799aea37d96b303) [`5787aa8`](https://github.com/muutot/Clipboard/commit/5787aa815cc5996b9ad9056ffe65b3bf244c9983)
- **标签** — 标签添加信号按边沿触发，重命名失败可见，取消/失败后恢复重命名输入 | [`300bf92`](https://github.com/muutot/Clipboard/commit/300bf925670b303b01c9b9cedfb907669f8428df) [`97c61f1`](https://github.com/muutot/Clipboard/commit/97c61f1ed8fa0afd61c75efc7f935ba8cb260db3) [`0d40a41`](https://github.com/muutot/Clipboard/commit/0d40a410f04df83d629da6c72a505f03e989b12c) [`4b4627f`](https://github.com/muutot/Clipboard/commit/4b4627f2535da73f61d37b12a7d8e37f1127dfad)
- **详情与查看器** — 重命名后刷新派生文件字段，切换条目重置标签草稿/标题草稿，无关更新不触发文件预览重载，右键子菜单贴边左翻 | [`7b8e28a`](https://github.com/muutot/Clipboard/commit/7b8e28a760965e9356b50ba4187659aeee4e617c) [`49a9a9f`](https://github.com/muutot/Clipboard/commit/49a9a9fe66b3c798c2c75f3bede38c71f2314610) [`7f05eba`](https://github.com/muutot/Clipboard/commit/7f05eba160e5852c39227e08946c94cbf02512dc) [`83942b3`](https://github.com/muutot/Clipboard/commit/83942b3242d064cba39d7091821438c3e06ca8b4) [`930434f`](https://github.com/muutot/Clipboard/commit/930434fcc2e017af3cd37b2fd196e90cd1c1a36c)
- **卡片高度与列表** — 高度签名纳入标签/文件名/物化，自定义标题卡片走共享公式，宽 token 折行计入估算 | [`a4527f0`](https://github.com/muutot/Clipboard/commit/a4527f01265e479a0a819894b4570043d9052b7d) [`5cfb6ac`](https://github.com/muutot/Clipboard/commit/5cfb6ac0571bd1c6035e5777f3f172a7f54b2959) [`23648df`](https://github.com/muutot/Clipboard/commit/23648df9f27947343c0460b8fcaf824521ed8975)
- **复制与文件展示** — 单文件路径列表可复制，无可复制时 toast，双文件条目显示双名与正确后缀计数 | [`dfd04e6`](https://github.com/muutot/Clipboard/commit/dfd04e671d1d67bc36553f1628e2cabe1ed034ce) [`90c9252`](https://github.com/muutot/Clipboard/commit/90c9252804bf0665c5e484828efe1ba39469a28c)
- **悬浮与菜单** — 悬浮面板实时套用通用设置，复制/粘贴统一单 toast，Escape 让位于打开的右键菜单 | [`cf48078`](https://github.com/muutot/Clipboard/commit/cf480789f00d5a694c231dc8a9023ea4323b2739) [`1c20c90`](https://github.com/muutot/Clipboard/commit/1c20c9093e85383758452971b94ca22aa5b8e66e) [`f911f76`](https://github.com/muutot/Clipboard/commit/f911f76799b8e3b058e7105f3660afc58edf7dcb)
- **事件与日志** — UI 刷新信号 emit 失败落日志，悬浮位置与同步状态写失败落日志 | [`e068518`](https://github.com/muutot/Clipboard/commit/e0685182a14f4be32dc034ab3d5c936cf1c7bc43) [`9e77f8f`](https://github.com/muutot/Clipboard/commit/9e77f8f398510c960de33112c6dc23fcd9b3e6e9)
- **设置语言同步** — 各 tab 同步 locale，持久化后清除 dirty keys | [`2be0d53`](https://github.com/muutot/Clipboard/commit/2be0d53dbe4918233b04d2ab716250cb48a1e40f)
- **文本编辑预览** — 文本编辑预览按重载时 `buildPreview` 规则派生 | [`dc19da1`](https://github.com/muutot/Clipboard/commit/dc19da1f45953623197ff90292f3a52e553dae19)

---

## OCR

- **tesseract 探测** — 探测次数设界、语言代码消毒，探测前释放配置锁 | [`434c7bc`](https://github.com/muutot/Clipboard/commit/434c7bc7e1e16d1d93276e1b7772c88d7f6fb63b) [`15f083b`](https://github.com/muutot/Clipboard/commit/15f083ba18b26eb53401c0168a67ac96328ecb06)

---

## 安装、构建与发布

- **安装器拒绝静默降级** — 解析已安装版本后拒绝降级安装 | [`a84db0a`](https://github.com/muutot/Clipboard/commit/a84db0adef06290251a9a08eb3dc8f0df9a11093)
- **发布直发 + GitCode 同步** — 发布直接上架，并同步镜像到 GitCode | [`a22fe0f`](https://github.com/muutot/Clipboard/commit/a22fe0f463a01a95a453552fc3e4e2bae57a825f)
- **下载修复** — 服务器忽略 Range 时重置部分下载 | [`81a3bcb`](https://github.com/muutot/Clipboard/commit/81a3bcb8d425840dc2395e260163c27f135debd1)
- **CI** — 覆盖 workspace 成员，lockfile 检查用 `npm ci` | [`cc31e2f`](https://github.com/muutot/Clipboard/commit/cc31e2f848bc9c0f84646914042c536de47b6b07)
- **依赖** — vitest 升至 5，npm 依赖升至最新 patch/minor | [`7eaa689`](https://github.com/muutot/Clipboard/commit/7eaa689b8715a1a596952c5147cd19939e0c8870) [`2334018`](https://github.com/muutot/Clipboard/commit/2334018b9e8bca773fbde2ec44d3186ca225e683)
- **杂项** — 调试构建禁止自重启，平台层清理既有 clippy `-D warnings` 失败，发布说明对话框全宽 | [`8e1deb6`](https://github.com/muutot/Clipboard/commit/8e1deb644e85282c9a8aafa9a5e17ce179abbcef) [`9ace15a`](https://github.com/muutot/Clipboard/commit/9ace15afc604aa0e8975f12fc543d7162617f6c5) [`0dc40fb`](https://github.com/muutot/Clipboard/commit/0dc40fb19cd3dc2ec82a0e2defdea67808ddf7c3)
- **文档** — README 增加 UI 截图与工具栏样式对比 | [`64de025`](https://github.com/muutot/Clipboard/commit/64de025a9a6ae24538225f925e78f2d4191f4d1e)

---

## 构建产物

- **MSI 安装包**: `Clipboard_1.7.0_x64_en-US.msi`
- **NSIS 安装包**: `Clipboard_1.7.0_x64-setup.exe`
