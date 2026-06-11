# Pulse - Universal API Monitor

一款基于 Tauri v2 + Rust + React 构建的跨平台 API 监控任务栏工具。

## 功能特性

- **系统托盘常驻** — 最小化到系统托盘，左键切换仪表盘，右键快捷菜单
- **实时健康检查** — 可配置的 HTTP 端点轮询，支持 GET/POST/PUT/HEAD/DELETE 方法
- **响应时间追踪** — 每次检查记录响应时间，SVG 趋势图可视化历史数据
- **可用率统计** — 自动计算各服务的 uptime 百分比和平均响应时间
- **状态告警** — 服务异常或恢复时弹出 Toast 通知
- **灵活配置** — 自定义检查间隔、超时时间、期望状态码、请求头和请求体
- **数据持久化** — 配置和历史自动保存到本地 JSON 文件
- **暗色主题** — 现代化深色 UI 设计，护眼且专业

## 技术栈

| 层 | 技术 |
|---|------|
| 框架 | Tauri v2 |
| 后端 | Rust (reqwest + tokio 异步 HTTP 监控) |
| 前端 | React 19 + TypeScript + Vite |
| 状态管理 | React hooks + Tauri events |
| 图表 | 自定义 SVG (零外部依赖) |
| 数据持久化 | JSON 文件 (AppData 目录) |

## 项目结构

```
pulse/
├── package.json                 # 前端依赖
├── vite.config.ts               # Vite 构建配置
├── tsconfig.json                # TypeScript 配置
├── index.html                   # HTML 入口
├── src/                         # 前端源码
│   ├── main.tsx                 # React 入口
│   ├── App.tsx                  # 主组件 (仪表盘/服务管理/设置)
│   └── App.css                  # 全局样式 (暗色主题)
└── src-tauri/                   # Rust 后端
    ├── Cargo.toml               # Rust 依赖
    ├── tauri.conf.json          # Tauri 配置
    ├── capabilities/
    │   └── default.json         # 权限声明
    ├── icons/                   # 应用图标
    └── src/
        ├── main.rs              # 入口
        ├── lib.rs               # 应用组装 (插件/托盘/监控/命令)
        ├── config.rs            # 配置模型和持久化
        ├── store.rs             # 运行时状态管理
        ├── monitor.rs           # 异步 HTTP 监控引擎
        ├── commands.rs          # Tauri invoke 命令
        └── tray.rs              # 系统托盘 (菜单/图标/状态)
```

## 快速开始

### 环境要求

- Rust 1.75+ (推荐 1.96+)
- Node.js 18+
- npm 或 pnpm

### 开发模式

```bash
# 安装前端依赖
npm install

# 启动开发服务器 + Tauri 应用
npm run tauri dev
```

### 构建生产版本

```bash
# 构建前端
npm run build

# 构建 Tauri 应用 (生成 .exe 安装包)
npm run tauri build
```

## 使用说明

1. **添加服务** — 切换到 "Services" 标签页，点击 "Add Service"，填入名称、URL 和监控参数
2. **查看状态** — "Dashboard" 标签页实时展示所有服务的健康状态、响应时间和趋势图
3. **手动检查** — 点击服务卡片上的 ↻ 按钮或顶部 "Refresh All" 立即触发检查
4. **配置通知** — 在 "Settings" 中开关系统通知、设置自启动和历史保留天数

## 参考

本项目的设计灵感来源于 [DeepSeekMonitor](https://github.com/JayHome137/DeepSeekMonitor) (macOS 原生菜单栏应用)，将其优秀的监控 UX 设计泛化为跨平台通用方案。

## License

MIT
