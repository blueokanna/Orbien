---
sidebar_position: 6
sidebar_label: Domains
title: Domains
---

# Domains

Forward visitor traffic to a client tunnel by domain. After the server enables `httpGwPort` / `httpsGwPort`, the client binds domains with `domains`.

`domains` is required and must be non-empty for http/https. Rules:

- A short name with no `.` (e.g. `web`) is treated as a subdomain prefix and expanded to `{prefix}.{rootDomain}` (requires server `rootDomain`)
- A full domain with `.` (e.g. `web.example.com`): if it belongs to `rootDomain` or a subdomain of it, it is used as-is; otherwise it is treated as an external custom domain and also used as-is
- Wildcards are not supported

**Port isolation:** `httpGwPort` / `httpsGwPort` must not share overlapping addresses with the control channel `listen` (`0.0.0.0` overlaps any address). The two gateway ports must also differ from each other. The control plane and visitor traffic use separate listeners; there is no protocol demux on a single port.

## Example: Custom domain

Server:

```toml
# orbien-server.toml
httpGwPort = 80
httpsGwPort = 443
```

Client:

```toml
# orbien.toml
[[tunnels]]
name = "web"
protocol = "http"
service = "127.0.0.1:8080"
domains = ["web.example.com"]
```

Point `web.example.com` at the server IP, then access it.

## Example: Subdomain prefix

Server:

```toml
# orbien-server.toml
httpGwPort = 80
rootDomain = "example.com"
```

Client:

```toml
# orbien.toml
[[tunnels]]
name = "web"
protocol = "http"
service = "127.0.0.1:8080"
domains = ["web"]
```

The access domain is `web.example.com`. You can also mix prefixes and full domains, e.g. `domains = ["web", "app.example.com"]`.

## Parameters

| Parameter     | Required | Default | Description                                                              |
|---------------|----------|---------|--------------------------------------------------------------------------|
| `httpGwPort`  | No       | `0`     | HTTP gateway port; `0` disables it; must not conflict with `listen`      |
| `httpsGwPort` | No       | `0`     | HTTPS gateway port; `0` disables it; must not conflict with `listen`     |
| `rootDomain`  | No       |         | Root domain; client `domains` entries with no `.` become `prefix.rootDomain` |
