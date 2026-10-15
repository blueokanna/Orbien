---
sidebar_position: 6
sidebar_label: 域名
title: 域名
---

# 域名

按域名将访客流量转发到客户端隧道。服务端开启 `httpGwPort` / `httpsGwPort` 后，客户端用 `domains` 绑定域名。

`domains` 对 http/https 必填且非空。规则：

- 不含 `.` 的短名字（如 `web`）当作子域前缀，展开为 `{前缀}.{rootDomain}`（需配置服务端 `rootDomain`）
- 带 `.` 的完整域名（如 `web.example.com`）：若属于 `rootDomain` 或其子域，按原样使用；否则当作外部自定义域名，同样按原样使用
- 不支持通配符

**端口隔离：** `httpGwPort` / `httpsGwPort` 不得与控制通道 `listen` 在重叠地址上共用（`0.0.0.0` 与任意地址视为重叠）。两者也不得彼此相同。控制面与访客流量使用独立监听，不在单端口上做协议分流。

## 示例：自定义域名

服务端：

```toml
# orbien-server.toml
httpGwPort = 80
httpsGwPort = 443
```

客户端：

```toml
# orbien.toml
[[tunnels]]
name = "web"
protocol = "http"
service = "127.0.0.1:8080"
domains = ["web.example.com"]
```

将 `web.example.com` 解析到服务端 IP 后访问。

## 示例：子域名前缀

服务端：

```toml
# orbien-server.toml
httpGwPort = 80
rootDomain = "example.com"
```

客户端：

```toml
# orbien.toml
[[tunnels]]
name = "web"
protocol = "http"
service = "127.0.0.1:8080"
domains = ["web"]
```

访问域名为 `web.example.com`。也可同时写前缀和完整域名，例如 `domains = ["web", "app.example.com"]`。

## 参数

| 参数            | 必填 | 默认值 | 说明                                      |
|---------------|----|-----|-----------------------------------------|
| `httpGwPort`  | 否  | `0` | HTTP 网关端口；`0` 表示关闭；不得与 `listen` 冲突 |
| `httpsGwPort` | 否  | `0` | HTTPS 网关端口；`0` 表示关闭；不得与 `listen` 冲突 |
| `rootDomain`  | 否  |     | 根域；客户端 `domains` 中无 `.` 的前缀会拼成 `前缀.根域` |
