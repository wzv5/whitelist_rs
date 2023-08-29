# nginx 动态 IP 白名单

通过动态生成 nginx 配置的方式来实现 IP 白名单。

本项目是 [whitelist(C#)](https://github.com/wzv5/WhiteList) 使用 rust 重写的版本，功能完全相同，配置文件不兼容，相信会比 C# 版更加稳定和节省资源。

（当然这依然是个练手项目，也是我第一次写 rust 项目）

## 配置

配置文件的搜索顺序：

1. 环境变量 `APP_CONFIG` 指定的配置文件，需要包含文件名的完整路径
2. 当前工作目录下的 `config.json`
3. exe 同级目录下的 `config.json`

必要的配置（[config.json](/config.json)）：

``` json
{
    "listen": {
        "urls": [
            "127.0.0.1:8080",
            "[::1]:8080"
        ],
        "path": "/a"
    },
    "whitelist": {
        "token": "aaa",
        "nginx_conf": "Z:\\whitelist.conf",
        "nginx_exe": "D:\\scoop\\home\\apps\\nginx\\current\\nginx.exe"
    }
}
```

完整配置（[config.full.json](/config.full.json)）：

``` json
{
    "listen": {
        "urls": [
            "127.0.0.1:8080",
            "[::1]:8080"
        ],
        "path": "/a",
        "allow_proxy": true
    },
    "whitelist": {
        "token": "aaa",
        "nginx_conf": "Z:\\whitelist.conf",
        "nginx_exe": "D:\\scoop\\home\\apps\\nginx\\current\\nginx.exe",
        "remote_addr_var": "my_real_ip",
        "result_var": "ip_whitelist",
        "timeout": 3600,
        "loop_delay": 15,
        "ipv4_prefixlen": 0,
        "ipv6_prefixlen": 0,
        "preset": [
            "127.0.0.0/8",
            "192.168.1.1"
        ]
    },
    "message": {
        "bark": ""
    },
    "baidu_location": {
        "ak": "",
        "referrer": ""
    }
}
```

说明：

1. `allow_proxy`：是否开启代理支持，开启后将从相关 http 头中获取远程地址，默认为 `true`
2. `remote_addr_var`：nginx 配置文件中表示远程地址的变量名，默认为 `remote_addr`
3. `result_var`：nginx 配置文件中保存结果的变量名，默认为 `ip_whitelist`，如果 `remote_addr` 在白名单中，该变量值为 `1`
4. `timeout`：成功提交后保留多久，单位为秒，默认 `3600`
5. `loop_delay`：多久检查一次列表，为了避免频繁重载 nginx 配置，提交成功和过期都不是实时的，默认为 `15`
6. `ipv4_prefixlen`：成功提交后，把该范围内的 IP 都加入白名单，默认为 `0`，等同于 `32`
7. `ipv6_prefixlen`：同上
8. `preset`：预置的白名单，始终会包含这些 IP 或 IP 段
9. `bark`：消息通知接口，不含最后的 `/`
10. `ak`：百度地图 API，用于获取 IP 的地理位置，仅在设置了 `bark` 、发送消息时使用
11. `referrer`：调用百度地图 API 时的 referrer，参见百度地图 API 文档的来源白名单

## 开启日志

使用 rust 的 env_logger 库管理日志，所以需要通过环境变量来设置日志。

``` bash
$ export RUST_LOG=info,whitelist_rs=debug
$ ./whitelist_rs
```

这将设置默认日志级别为 info，当前程序的日志级别为 debug。

## systemd 服务

1. 修改 `whitelist_rs.service` 文件中的相关路径
2. 运行 `up.sh` 安装服务并开启
3. 运行 `log.sh` 查看日志

## nginx 配置

``` nginx
server {
    location = /a {
        proxy_redirect      off;
        proxy_pass          http://127.0.0.1:8080;
        proxy_set_header    Host $host;
        proxy_set_header    X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header    X-Forwarded-Proto $scheme;
    }
}
```

在需要白名单的地方：

``` nginx
include whitelist.conf;

server {
    location = /xxx {
        if ($ip_whitelist != 1) {
            return 403;
        }
    }
}
```

## 把自己的 IP 加入白名单

1. 手动访问 `http://.../a`，在页面中填写 token
2. 或者，直接向 `http://.../a` 发送 POST 请求，参数为 `token=xxx`
3. 成功会看到 `hello`，最多 15 秒后白名单即可生效
4. 成功一次将保持 1 小时，超时后会自动清除，需要再次提交

## 增加安全性

1. 强烈建议使用 https
2. 对于爆破，日志中会输出相关信息，可以配合使用 fail2ban 自动拉黑 IP
