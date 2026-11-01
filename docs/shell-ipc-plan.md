# 外壳与核心之间的进程通信：方案设计

状态：**已按方案 B 落地**（2026-11-01，分支 `strip-electron` 工作区）。落地后的实际形状、与原设计的偏差、没做的部分见 §11。
涉及代码：`shell-rs/src/bridge.rs`、`host-remote.js`、`start.js`、`shell-rs/src/supervisor.rs`、`scripts/ipc-call.mjs`。
日期：2026-10-31 设计，2026-11-01 落地。

## 0. 一句话结论

可以，而且值得做：把「外壳 ↔ 核心」这条控制通道从环回 HTTP 换成**本地套接字上的长度前缀 JSON 帧**（方案 B：Windows 命名管道，macOS / Linux 的 Unix 域套接字）。
管理页那条 HTTP 端口必须保留 —— 浏览器只会说 HTTP，这条通道不在本文的改造范围内。

## 1. 现在到底有几个端口

| 通道 | 谁监听 | 谁连接 | 地址 | 鉴权 | 干什么 |
|---|---|---|---|---|---|
| 管理页 | 核心 `server.js` | 浏览器、dsh 实例页 | `http://127.0.0.1:3780`（`settings.json` 的 `port`，被占则顺延） | 无（只绑环回） | 整个管理界面与 `/api/*` |
| 外壳桥 | 外壳 `bridge.rs` | 核心 `host-remote.js` | `http://127.0.0.1:<随机端口>`（`Server::http("127.0.0.1:0")`） | `Authorization: Bearer <32 位随机 token>` | 核心回调外壳的 host 成员 |

调用点与契约（当前实现）：

- **核心 → 外壳**：`host-remote.js:63` 的 `request()` 发 `POST <base>/host/<path>`；路由全表在 `bridge.rs:87-151`：`open` / `wake` / `notify` / `exit` / `relaunch` / `info` / `desktop/{notify,open,reveal,clipboard,dialog}` / `browser/{clearCache,clearAll}` / `shellUpdate/{state,check,download,install}`。
- **外壳 → 核心**：`POST /api/internal/shell-update`（`server.js:6012`，用同一个 token 校验，`server.js:6013`）。**注意：今天没有任何代码调用它** —— 外壳的 `updater.rs` 只返回 `disabled`，这条反向通道是为将来的外壳自更新预留的。
- **环境变量契约**（`supervisor.rs:210-216`）：`DSH_SHELL_URL` / `DSH_SHELL_TOKEN` / `DSH_SHELL_PID` / `DSH_SHELL_EXE` / `DSH_SHELL_VERSION` / `DSH_SHELL_PREFS`。
- **生命周期**：核心每 3 秒 `process.kill(DSH_SHELL_PID, 0)`（`start.js:86` 的 `watchShell()`），外壳没了就自己收摊；反过来外壳用 `supervisor.rs` 起停核心。

> 上面这张表和四条契约是**改造前**的形状，留作对照；改造后的实际契约见 §11。

## 2. 为什么想换（按真实痛点排序）

1. **多一个监听、多一个端口**。它只服务两个进程，却要走完整的 TCP 监听路径：`127.0.0.1:0` 随机挑端口 → 写进环境变量 → 核心再读回来。端口被占、监听失败、安全软件记「程序在监听端口」，都是这条链上的额外失败模式。
2. **秘密在环境变量里**。token 能证明「知道这串字符」，证明不了「你就是我拉起来的那个进程」。同用户的任何进程都能读环境变量、也能撞上环回端口；今天的 403 只是提高门槛，不是身份校验。
3. **生命周期只能靠轮询**。核心不知道外壳还在不在，只能每 3 秒问一次系统；外壳被强杀时，核心最长会多活 3 秒，期间还占着管理页端口与文件（卸载残留就是这么来的）。管道/UDS 的 EOF 是即时且精确的死亡信号。
4. **两个方向两种写法**。核心手写 HTTP 客户端（超时、`AbortSignal`、JSON 解码、吞错），外壳手写 HTTP 服务端（头、状态码、body），反向又走核心的 HTTP 路由 —— 同一件事在三个地方各写一遍。
5. **三条补丁是 HTTP 的产物**：`DEFAULT_TIMEOUT_MS = 15_000`、`CACHE_TTL_MS = 5_000`、`refreshSoon()` 的去重。前两条因为「跨进程调用可能卡住」，第三条因为「同步成员不能等网络」。换成管道后缓存还得留（同步 API 不变），但超时可以收紧、失败能立刻知道。
6. **代码量**：`bridge.rs` 267 行里约 60 行在处理 HTTP（`tiny_http` 只解决了一半，路由、鉴权、错误码仍要手写）；`host-remote.js` 183 行里约 40 行在超时/解码/吞错。

也要诚实地说两条「不是理由」的：

- **延迟不是动机**。本机 HTTP 一次往返 0.3–2 ms，管道 0.05–0.2 ms，人感觉不到；对话框、通知、开浏览器这些操作本身的耗时是几十毫秒到几百毫秒。
- **防火墙不是决定性理由**。只绑 `127.0.0.1` 通常不会弹 Windows 防火墙提示；但部分 EDR / 安全软件会把「程序在监听端口」记成可疑行为，这在企业环境里是真实噪音。

## 3. 可行性边界（决定哪些方案能上）

- **核心零运行时依赖**：只能用 `node:net` / `node:http` / `node:fs` / `node:crypto`。→ 一切需要 native addon 的方案直接出局（共享内存、FFI 窗口消息）。
- **Node 22 与 24 都要支持**：命名管道走 `\\.\pipe\...`，Unix 域套接字走文件路径，两者都在 `node:net` 里，不需要新依赖。
- **外壳与核心分开更新**：新核心要能配旧外壳、新外壳要能配旧核心（同一个 `DSH_SHELL_*` 契约内做协商），否则用户升级到一半就起不来。
- **核心必须还能单独跑**（`npm start` 浏览器模式）：不能把「必须有外壳」写进启动路径。
- **管理页必须是 HTTP**（浏览器限制），这条不改。
- **三平台一致**：Windows / macOS / Linux，Linux 还要分 AppImage 与 deb 两种装法（可写路径不同）。

本机已实测（Node，Windows，2026-10-31）：

- `net.createServer` 监听命名管道 `\\.\pipe\dsh-x-ipc-test-<pid>` 成功（Node 的 `listen(path)` 直接吃管道名）；
- `http.request({ socketPath: 同一个管道名 })` 也跑通了：收到 `200` 与响应体（我手写的响应把 `Content-Length` 写错成 15、正文 16 字节，才报了一次 `Parse Error: Data after Connection: close`，与管道无关）。

→ 方案 B / C 的 **Node 侧没有未知数**；未知数都在 Rust 侧（见 6.1 的 spike）。

## 4. 候选方案

每个方案统一给：机制 / 协议 / 两端改动 / 优点 / 代价 / 结论。

### 方案 A：复用子进程 stdio（帧化管道）

**机制**：外壳 `spawn` 核心时本来就连着 stdin / stdout。把 `stdin` 当「外壳 → 核心」请求流，`stdout` 当「核心 → 外壳」响应流。

**协议**（行分隔，用 `\x1e` 记录分隔符避开日志）：

```
<RS>{"id":7,"method":"info"}
{"id":7,"ok":true,"result":{"versions":{"shell":"0.1.24","node":"24.19.0"}}}
```

**两端改动**：

- 核心：`host-remote.js` 的 transport 换成「写一行 + 按 id 等响应」，`start.js` 用 `node:readline` 读 `process.stdin`。
- 外壳：`supervisor.rs` 给 `stdin(Stdio::piped())`，加一个响应读线程与 `id → mpsc` 的待回复表（约 100 行）。

**优点**：零新增依赖、零监听、天然绑定生命周期（EOF 即死）、三平台同一套代码。

**代价**：**日志与控制流复用同一条管道**。`server.js` 的 `pushLog` 直接写 stdout，外壳又从 stdout 上解析 `dsh-versions: <url>`（`supervisor.rs:343` 起）；dsh 子进程继承 stdout 后，任何子进程写半行都会把帧切碎。要做干净就得把日志整体挪到 stderr，而 stderr 现在是「报错专用」、面板的日志尾巴取的是 stdout —— 波及 `pushLog`、`booting.html` 的进度与既有用例。

**结论**：可行但脏。只有在「不想加任何 crate、也不想碰套接字」时才选。

### 方案 B（推荐）：本地套接字 + 长度前缀 JSON 帧

**机制**：Windows 命名管道 `\\.\pipe\dsh-x-<pid>-<随机>`；mac/Linux Unix 域套接字 `<APP_DIR>/ipc/shell-<pid>.sock`。方向与今天一致：**外壳当服务端、核心当客户端**，调用方代码不用反过来。

**名字怎么传**：外壳起好监听后把名字写进 `DSH_SHELL_IPC`，核心优先用它；没有才回落 `DSH_SHELL_URL`（HTTP，保留一轮过渡）。

**协议**：

```
帧   := 4 字节小端长度（上限 1 MiB） + UTF-8 JSON
请求 {"id":7,"method":"notify","params":{...}}
响应 {"id":7,"ok":true,"result":{...}} 或 {"id":7,"ok":false,"error":"..."}
事件 {"event":"shellUpdate","data":{...}}   （无 id，留给将来的外壳更新推送）
```

一条连接、全双工；核心是唯一发起方（今天就是），事件方向留给反向推送。

**鉴权**：连接建立后**校验对端 pid** —— Windows `GetNamedPipeClientProcessId`，Linux `SO_PEERCRED`，mac `LOCAL_PEERPID`（退一步至少有 0600 的文件权限）。这比 bearer token 强一档：token 证明「知道秘密」，pid 证明「你就是我拉起来的那个进程」。管道名里的随机串只防误连，不作为安全边界。

**权限**：Windows 创建时传只含当前用户 SID 的 `SECURITY_ATTRIBUTES`，并带 `FILE_FLAG_FIRST_PIPE_INSTANCE`（防别人抢先建同名管道）；unix 目录 0700、套接字 0600，退出时删文件。

**两端改动**：

- 外壳：`bridge.rs` 的 `match route` 本来就是纯函数式的映射，抽成 `handle_route(route, input) -> Value` 后原位复用，一行业务逻辑都不用改；外面换成 `accept → 读帧 → 派发 → 写帧`，删掉 `tiny_http`。Windows 侧二选一：`interprocess` crate（一个 API 覆盖管道与 UDS），或者用已依赖的 `windows-sys` 手写（`CreateNamedPipeW` / `ConnectNamedPipe` + `File` 读写，约 120 行）。mac/Linux 用 `std::os::unix::net::UnixListener`，零依赖。
- 核心：`host-remote.js` 的 `request()` 换成「写帧 + 按 id 等回包」，`call()` 的 fallback、同步成员的缓存全部原样保留。约 80 行，对外形状不变。

**优点**：少一个端口；秘密从 token 变成对端身份；EOF 就是外壳死亡信号（watchdog 从 3 秒轮询降级为兜底）；三平台一套协议；路由与业务逻辑零改动。

**代价**：多一条传输实现要维护；不能再用 `curl` 调试（补一个 `scripts/ipc-call.mjs`）；Windows 上要在「加 crate」和「手写 120 行」之间选。

**结论**：**推荐**。

### 方案 C：本地套接字 + 原样 HTTP（换传输不换协议）

**机制**：还是 `POST /host/<path>` + Bearer，只把 TCP 换成管道。核心侧 `http.request({ socketPath })` 实测可用，几乎不用改（换掉 `base` 的求值方式）；外壳侧要写一个「管道上的 HTTP/1.1 服务端」（`BufReader` 读请求行与头、定长 body、强制 `Connection: close`），约 100 行，替掉 `tiny_http`。

**优点**：核心侧改动最小；状态码语义与既有用例都能保留（`test/host-remote.test.mjs` 的假桥只换传输）。

**代价**：外壳要手写 HTTP 解析（头的大小写、超长行、`Content-Length`、半包），这是最容易在边界上出错的部分；鉴权仍是 token，拿不到 pid 绑定；HTTP 的一半复杂度仍然在。

**结论**：**最小改动备选**。想先解决「端口」问题、其余以后再说时选它。

### 方案 D：保留 HTTP，加对端身份校验

**机制**：仍绑 `127.0.0.1:<随机>`，accept 之后用 `GetExtendedTcpTable(TCP_TABLE_OWNER_PID_ALL)` 把客户端端口映射到 pid，与核心 pid 比对；unix 用 `SO_PEERCRED`。token 退化成第二因子。

**优点**：约 40 行，安全上去了（不再靠秘密）。

**代价**：端口还在、轮询还在，痛点 1 / 3 / 5 一个都没解决。

**结论**：只作为「不想动传输」的补丁，不推荐单独做。

### 方案 E：共享内存 + 门铃事件

**机制**：`CreateFileMapping` / POSIX `shm_open` 放环形缓冲，命名事件或信号量做门铃，消息零拷贝。

**否决理由**：**核心侧做不到零依赖** —— Node 没有 mmap 与命名共享内存的 API，要么写 native addon，要么塞一个 wasm 消息队列（还得靠 IO 通知）。为了省 0.1 ms 引入 native 编译，与「核心零运行时依赖、不编译」的前提直接冲突。

**结论**：否决。

### 方案 F：文件邮箱（目录 + 原子改名 + 轮询）

**机制**：核心写 `requests/<id>.json`，外壳用 `notify` 监听目录；响应写 `responses/<id>.json`。本质是把 `shell-url.txt` 那套放大。

**优点**：不碰套接字，最不容易被安全软件拦；实现约 60 行。

**代价**：延迟 50–200 ms，对「打开浏览器」「弹对话框」是可感知的；要处理半写文件、清理、并发，机器上还多一个目录要维护。

**结论**：**最后手段**（企业环境连命名管道都禁时），可当方案 B 的二级自动降级目标。

### 方案 G：Windows 窗口消息 / COM

**机制**：`WM_COPYDATA` 发到外壳窗口，或注册 COM 本地服务器。

**否决理由**：核心是 Node，发窗口消息要 FFI（`koffi` 之类）→ 违反零依赖；COM 还要注册表注册与类型库。外壳侧简单，但只解决 Windows。

**结论**：否决（除非将来允许核心带 native 模块）。

### 方案 H：合端口 + 长轮询（只保留一个 HTTP 端口）

**机制**：不新增传输，把桥挂到管理页同一个端口（`/api/internal/host/*`，token 校验）；外壳启动后开一条 SSE 长连接收「核心 → 外壳」的指令，回执走同一个 POST。

**优点**：端口从 2 个变 1 个；不用写新传输；Node 的 HTTP 栈直接复用。对「别多开一个监听」这个诉求够用。

**代价**：仍然是 HTTP + token；要写 SSE 重连与指令去重；外壳崩了还是靠 watchdog；服务端/客户端关系翻过来后，`bridge.rs` 里那些同步能力（对话框、剪贴板）要改成「收指令 → 回执」，多一层状态机。

**结论**：**如果目标只是「少一个端口」**，这是性价比最高的一条；但它回答不了「不要用 HTTP」。

### 方案 I：把核心塞进外壳进程（进程合并）

**机制**：外壳内嵌 Node（`libnode`）直接 `import` 核心的 `server.js`，像 Electron 那样一个进程。

**否决理由**：要打包 V8、三平台各自编译、体积与许可证都上一个台阶；而且把「外壳崩了核心跟着崩」的风险引回来，正好与 `DSH_SHELL_PID` 看门狗的设计意图相反。

**结论**：否决。

## 5. 对比

| 方案 | 端口 | 鉴权 | 生命周期 | 核心改动 | 外壳改动 | 跨平台 | 可调试 | 结论 |
|---|---|---|---|---|---|---|---|---|
| A stdio | 0 | 进程血缘 | EOF 即时 | 中（日志要分流） | 中 | 一致 | 差（帧混在日志里） | 备选 |
| B 套接字 + 帧 | 0 | pid + 文件权限 | EOF 即时 | 小到中 | 中 | 一致 | 中（要工具） | **推荐** |
| C 套接字 + HTTP | 0 | token | EOF 即时 | 极小 | 中 | 一致 | 好 | 备选（最小改动） |
| D TCP + pid | 1 | pid | 轮询 | 0 | 小 | 一致 | 好 | 补丁 |
| E 共享内存 | 0 | 进程血缘 | 事件 | 不可能（要 native） | 大 | 分平台 | 差 | 否决 |
| F 文件邮箱 | 0 | 目录权限 | 轮询 | 小 | 小 | 一致 | 好 | 降级手段 |
| G 窗口消息 | 0 | 窗口句柄 | 无 | 不可能（要 FFI） | 小 | 仅 Windows | 差 | 否决 |
| H 合端口 | 1 | token | 轮询 | 小 | 中 | 一致 | 好 | 只想少端口时选 |
| I 进程合并 | 0 | 无 | 同生共死 | 大 | 大 | 三平台都要编 | 差 | 否决 |

## 6. 推荐方案的落地细节（方案 B）

### 6.1 名字、权限、spike

- Windows：`\\.\pipe\dsh-x-<外壳 pid>-<8 位随机>`；`CreateNamedPipeW` 传只含当前用户 SID 的安全描述符，带 `FILE_FLAG_FIRST_PIPE_INSTANCE`。
- mac/Linux：`<APP_DIR>/ipc/shell-<pid>.sock`（`prefs::app_dir()` 已存在，在用户目录下），目录 0700、套接字 0600，退出时删。
- 环境变量新增 `DSH_SHELL_IPC=<名字>`；过渡期 `DSH_SHELL_URL` / `DSH_SHELL_TOKEN` 继续给。
- **先做一次 spike**（约 20 行）：`interprocess` crate 在 Windows 上能否 `Send + Sync` 跨线程、`read` 是否阻塞语义干净、UDS 是否同一套 API。spike 不过就退回 `windows-sys` 手写 + `UnixListener`。

### 6.2 帧与错误语义

- 4 字节小端长度 + JSON，上限 1 MiB：超了直接断连并记日志（别让一个恶意长度把内存吃光）。
- 超时：普通调用 10 秒（现在 15 秒），`exit` / `relaunch` 仍 2 秒（「发完就走」）。
- 错误从「HTTP 状态码 + `error` 字段」变成 `{ok:false,error}`；`call()` 的 fallback 语义、同步成员的缓存全部保留（同步 API 还是不能在管道上等）。

### 6.3 生命周期

- 连接 EOF → 核心立即退出，替代 3 秒轮询；`watchShell()` 保留成兜底（管道被安全软件掐断但进程还在的极端情况）。
- 外壳重启核心 → 管道名随 pid 变化，核心连新名字。

### 6.4 版本协商（必须双向兼容）

| 组合 | 行为 |
|---|---|
| 新外壳 + 旧核心 | 外壳同时起管道与 HTTP，旧核心只认 `DSH_SHELL_URL`，照旧 |
| 新核心 + 旧外壳 | 没有 `DSH_SHELL_IPC`，回落 `DSH_SHELL_URL` |
| 两个都没有 | 浏览器模式（`REMOTE_HOST` 为 null），不变 |

### 6.5 排障

- 加 `node scripts/ipc-call.mjs <method> [json]`：从环境变量取管道名，发一帧、打印回包。这是「没有 curl 之后」唯一顺手的验证手段，也进用例。
- 日志明写通道类型：`IPC 通道：<名字>` 或 `回落 HTTP：<url>`。

## 7. 分阶段（每步都可回退）

| 阶段 | 内容 | 估时 | 回退方式 |
|---|---|---|---|
| P0 抽层 | `host-remote.js` 抽出 transport（http / socket 两种实现，先用内存 transport 把既有 10 条用例跑通）；`bridge.rs` 抽 `handle_route()`，行为不变 | 0.5 天 | 纯重构，无需回退 |
| P1 双路 | 外壳起管道 + 帧编解码 + pid 校验；核心加 socket transport；env 两个都给，核心优先 IPC、失败回落 HTTP 并记一行日志 | 1.5 天 | 去掉 `DSH_SHELL_IPC` 即回旧路 |
| P2 切换 | 默认只给 IPC；`DSH_CORE_LEGACY_HTTP=1` 时才给 URL；真机跑托盘、通知、对话框、退出、重启、核心更新 | 1 天 | 置环境变量回旧路 |
| P3 收尾 | 删 `tiny_http`、`/api/internal/shell-update`、`DSH_SHELL_URL` / `DSH_SHELL_TOKEN`、watchdog 轮询（保留一次兜底检查） | 0.5 天 | 按 tag 回滚 |

合计 3–3.5 人日，不含三平台各一次真机验收。

## 8. 验收

- **单测**：帧编解码（粘包、半包、超 1 MiB、非法 JSON）、超时、对端 pid 不符、对端消失、回落 HTTP。
- **既有用例**：`test/host-remote.test.mjs` 的 10 条换成 pipe 版假桥（Windows 用唯一管道名，mac/Linux 用临时目录下的 sock），断言形状不变。
- **真机**：起外壳 → `/api/self` 的 `shell.current` 有值且日志显示「IPC 通道」；托盘「打开管理页」开浏览器；`POST /api/desktop/notify` 弹通知；插件对话框能弹；`/api/internal/shell-update` 不再存在；`taskkill` 外壳后核心**秒退**（不再是 3 秒）；杀核心后外壳自动重启并重连。
- **三平台**：Windows（命名管道）、macOS（UDS + `.app` 下的 `APP_DIR`）、Linux（AppImage 与 deb 各一次，注意 `$XDG_RUNTIME_DIR` 不存在时回退到 `APP_DIR`）。

## 9. 风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| 安全软件拦命名管道 | 核心与外壳失联 | 保留一版 HTTP 回退；方案 F 作二级降级 |
| 帧编解码写错（半包/粘包） | 偶发卡死，比 HTTP 难查 | 长度前缀 + 上限 + 超时 + 单测覆盖半包 |
| 新核心配旧外壳 | 起不来 | env 双给 + 回落 |
| mac `.app` 路径不可写 | 通道建不起来 | 用 `prefs::app_dir()`（用户目录，已存在）；失败回落 HTTP |
| 排障手段变少 | 现场难定位 | `scripts/ipc-call.mjs` + 日志明写通道类型 |

## 10. 非目标

- 管理页端口与 `/api/*` 不动 —— 浏览器只能说 HTTP。
- dsh 实例与核心之间不动（那是 dsh 自己的 HTTP / WebSocket，不属于本仓库）。
- 不做跨机器、跨用户的通道；不升级信任模型（仍是「同用户本机」）。

## 11. 落地记录（2026-11-01）

### 11.1 实际形状

| 项 | 设计（§6） | 实际落地 |
|---|---|---|
| 名字 | Windows `\\.\pipe\dsh-x-<pid>-<8 位随机>`；unix `<APP_DIR>/ipc/shell-<pid>.sock` | 一致；unix 侧 `create_dir_all` 后把目录设 0700、套接字 0600，bind 前先删旧文件 |
| 库 | `interprocess`（一个 API 覆盖管道与 UDS）或 `windows-sys` 手写 | `interprocess = "2.4.7"`（不加 async/tokio feature），`tiny_http` 已从 `Cargo.toml` 删掉 |
| 鉴权 | 对端 pid（`GetNamedPipeClientProcessId` / `SO_PEERCRED` / `LOCAL_PEERPID`） | `peer_creds().pid()` 与核心 pid 比对；核心 pid 由 `supervisor.rs` 每次 spawn 后写进 `AtomicU32`，0 时放行（外壳刚起、核心还没起的那几毫秒）；mac 的 xucred 拿不到 pid → 退回文件权限 |
| 帧 | 4 字节小端长度（≤1 MiB）+ UTF-8 JSON | 一致；`{id,method,params}` / `{id,ok,result\|error}` / `{event,data}` |
| 超时 | 普通 10 秒，`exit`/`relaunch` 2 秒 | 一致（`host-remote.js` 的 `DEFAULT_TIMEOUT_MS` / `EXIT_TIMEOUT_MS`） |
| 反向推送 | 事件帧 | 连接建立时外壳先推一次 `shellUpdate`（`bridge.rs` 的 `serve()`），核心 `ingest()` 合并；`server.js` 的 `POST /api/internal/shell-update` 已删 |
| 生命周期 | EOF → 核心立即退出，`watchShell()` 兜底 | 一致：`host-remote.js` 的 `onGone` → `start.js` 的 `shellGone()` → `shutdown()` 后 `process.exit(0)`；看门狗保留 3 秒兜底 |
| 排障 | `scripts/ipc-call.mjs` | 已加（复用 `createIpcTransport`，`--path` 或 `DSH_SHELL_IPC`）；它是独立进程，pid 对不上，所以外壳要用 `DSH_SHELL_IPC_ALLOW_ANY=1` 起才放行（默认严格，启动时 stderr 会提醒） |

### 11.2 与原设计的偏差

- **没有做 HTTP 过渡（P1/P2 的双路）**。Rust 外壳与 `@duqingyude/dsh-x-core` 都还没发布过，不存在「新核心配旧外壳」的存量组合，所以一次切干净：`DSH_SHELL_URL` / `DSH_SHELL_TOKEN` / `tiny_http` / `/api/internal/shell-update` 全部删除，`host-remote.js` 只剩 IPC 一种 transport。
- **Windows 的安全描述符与 `FILE_FLAG_FIRST_PIPE_INSTANCE` 没显式设置**。`interprocess` 用默认 DACL（当前用户可连），安全边界是靠对端 pid 校验 + 管道名里的随机串；要再做一层得等 `interprocess` 暴露安全描述符配置，或者退回 `windows-sys` 手写。
- **事件帧目前只有外壳连上时的那一次推送**。外壳更新器还没接（`updater.rs` 只返回 `disabled`），等接上时用同一个 `{event:'shellUpdate'}` 往连接里写即可，核心侧不用改。

### 11.3 验收结果

- **Rust 单测**：`cargo test --manifest-path shell-rs/Cargo.toml` 7 passed（新增帧编解码：按序两帧、帧边界 EOF、超 1 MiB 与空帧拒绝、坏 JSON 拒绝、8 位十六进制随机串）。
- **核心用例**：`test/host-remote.test.mjs` 14 条全绿——假外壳是 `node:net` 服务器（Windows 用唯一管道名，其它平台用临时目录下的 sock），覆盖请求/响应形状、半包拼接、超时、EOF、连不上、事件帧 ingest、`start.js` 结构断言。
- **真机（Windows）**：见 `docs/strip-electron-plan.md` §10 的「外壳通道」一行。

## 附：本机实测记录（2026-10-31，Windows）

```js
import net from 'node:net'
import http from 'node:http'

const pipe = String.raw`\\.\pipe\dsh-x-ipc-test-${process.pid}`
const server = net.createServer((sock) => {
  sock.once('data', () => {
    sock.end('HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{"ok":true,"a":1}')
  })
})
server.listen(pipe)                                  // 命名管道服务端：可用
http.request({ socketPath: pipe, path: '/host/info', method: 'POST' })  // HTTP over pipe：可用
```

结论：方案 B 与 C 的 Node 侧都不需要新增依赖；未知数集中在 Rust 侧，用 6.1 的 spike 一次性确认。
