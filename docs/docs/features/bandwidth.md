---
sidebar_position: 4
sidebar_label: 带宽限制
title: 带宽限制
---

# 带宽限制

按隧道限制转发带宽，配置在 `[[tunnels]]` 的 `transport` 下。

- `bandwidthLimitSide = "client"`：在客户端限速（默认）
- `bandwidthLimitSide = "server"`：在服务端限速

`bandwidth` 为数字，单位固定为Mbps（如 `2`、`0.5`）；`0` 表示不限制。

## 示例：客户端限速

```toml
# orbien.toml
[[tunnels]]
name = "web"
protocol = "tcp"
service = "127.0.0.1:80"
remotePort = 9000
transport.bandwidth = 2
transport.bandwidthLimitSide = "client"
```

## 示例：服务端限速

```toml
# orbien.toml
[[tunnels]]
name = "web"
protocol = "tcp"
service = "127.0.0.1:80"
remotePort = 9000
transport.bandwidth = 0.5
transport.bandwidthLimitSide = "server"
```

## 参数

| 参数                             | 必填 | 默认值      | 说明                          |
|--------------------------------|----|----------|-----------------------------|
| `transport.bandwidth`          | 否  | `0`      | 带宽上限（Mbps）；`0` 表示不限制       |
| `transport.bandwidthLimitSide` | 否  | `client` | 限速端：`client` / `server`     |
