---
sidebar_position: 8
sidebar_label: 日志
title: 日志
---

# 日志

支持将日志持久化到文件系统，默认是 **关闭**状态。

- 不配 `[log]` 或 `file`路径为空：只打终端日志
- 配置 `file`：同时写终端与文件
- 文件按天滚动，超过 `maxDays` 的备份会删除

:::info[说明]
日志相关配置仅在进程启动时生效，且 **不支持热重载**
:::

## 示例：客户端

```toml
# orbien.toml
[log]
file = "./orbien.log"
level = "info"
maxDays = 3
```

## 示例：服务端

```toml
# orbien-server.toml
[log]
file = "./orbien-server.log"
level = "info"
maxDays = 3
```

## 参数

| 参数      | 必填 | 默认值 | 说明                                          |
|-----------|------|--------|-----------------------------------------------|
| `file`    | 否   |        | 日志文件路径；空不会持久化                    |
| `level`   | 否   | `info` | `trace` / `debug` / `info` / `warn` / `error` |
| `maxDays` | 否   | `3`    | 备份保留天数；`0` 表示不清理                  |

