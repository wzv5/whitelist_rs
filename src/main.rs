#[macro_use]
extern crate log;

mod config;
mod service;

use bytes::Buf;
use futures_util::{future, FutureExt, TryFutureExt};
use hyper::{Body, Method, Request, Response, StatusCode};
use service::*;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};

type Error = Box<dyn std::error::Error + Send + Sync>;
type Result<T> = std::result::Result<T, Error>;

struct MyAppData {
    service: Mutex<WhiteListService>,
    token: String,
    allow_proxy: bool,
    path: String,
}

const APP_NAME: &str = env!("CARGO_PKG_NAME");

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", format!("info,{}=debug", APP_NAME));
    }
    env_logger::init();

    let cfg = config::load_config()?;

    if cfg.listen.allow_proxy {
        warn!("已开启代理支持，请注意防范远程地址伪造");
    }

    let listcfg = WhiteListServiceConfig {
        nginx_conf: cfg.whitelist.nginx_conf,
        nginx_exe: cfg.whitelist.nginx_exe,
        remote_addr_var: cfg.whitelist.remote_addr_var,
        result_var: cfg.whitelist.result_var,
        timeout: Duration::from_secs(cfg.whitelist.timeout.into()),
        loop_delay: Duration::from_secs(cfg.whitelist.loop_delay.into()),
        ipv4_prefixlen: cfg.whitelist.ipv4_prefixlen,
        ipv6_prefixlen: cfg.whitelist.ipv6_prefixlen,
        preset: cfg.whitelist.preset,
    };
    let mut msgsvc: Option<MessageService> = None;
    let mut locsvc: Option<BaiduLocationService> = None;
    if !cfg.message.bark.is_empty() {
        msgsvc = Some(MessageService::new(MessageServiceConfig {
            bark: cfg.message.bark,
        }));
    }
    if !cfg.baidu_location.ak.is_empty() && !cfg.baidu_location.referrer.is_empty() {
        locsvc = Some(BaiduLocationService::new(BaiduLocationServiceConfig {
            ak: cfg.baidu_location.ak,
            referrer: cfg.baidu_location.referrer,
        }))
    }
    let ctx = Arc::new(MyAppData {
        service: Mutex::new(WhiteListService::new(listcfg, msgsvc, locsvc)),
        token: cfg.whitelist.token,
        allow_proxy: cfg.listen.allow_proxy,
        path: cfg.listen.path,
    });

    let mut tasks = vec![];
    for addr in cfg.listen.urls {
        let ctx = ctx.clone();
        let srv = async move {
            tokio::spawn(listen_http(ctx, addr.parse().unwrap()))
                .await
                .unwrap()
                .unwrap();
        };
        tasks.push(srv.boxed());
    }
    future::join_all(tasks).await;

    Ok(())
}

async fn listen_http(ctx: Arc<MyAppData>, addr: SocketAddr) -> Result<()> {
    hyper::Server::bind(&addr)
        .http1_only(true)
        .serve(hyper::service::make_service_fn(
            |socket: &hyper::server::conn::AddrStream| {
                let remote_addr = socket.remote_addr();
                let ctx = ctx.clone();
                async move {
                    Ok::<_, Error>(hyper::service::service_fn(move |req| {
                        process_http(ctx.clone(), req, remote_addr).map_err(|err| {
                            error!("{}", err);
                            err
                        })
                    }))
                }
            },
        ))
        .await?;
    Ok(())
}

async fn process_http(
    ctx: Arc<MyAppData>,
    req: Request<Body>,
    remote_addr: SocketAddr,
) -> Result<Response<Body>> {
    let ua = req
        .headers()
        .get(hyper::header::USER_AGENT)
        .map(|ua| ua.as_bytes())
        .or(Some(&[]))
        .unwrap();
    let ua = String::from_utf8_lossy(ua);
    let ip = get_remote_ip(&req, &remote_addr, ctx.allow_proxy);
    info!(
        "{} \"{} {} {:?}\" \"{}\"",
        ip,
        req.method(),
        req.uri(),
        req.version(),
        ua
    );
    if req.uri().path() != ctx.path {
        return Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())?);
    }
    let (status, body) = match *req.method() {
        Method::GET => (StatusCode::OK, Body::from(get())),
        Method::POST => post(ctx, req, ip).await?,
        _ => (StatusCode::METHOD_NOT_ALLOWED, Body::empty()),
    };
    Ok(Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(body)?)
}

fn get() -> &'static str {
    r#"<!DOCTYPE html>
<html lang="zh">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <meta http-equiv="X-UA-Compatible" content="ie=edge">
    <title>登录</title>
</head>
<body>
    <form method="POST">
        <label for="token">token: </label>
        <input name="token" id="token"/>
        <button type="submit">提交</button>
    </form>
</body>
</html>
"#
}

async fn post(
    ctx: Arc<MyAppData>,
    mut req: Request<Body>,
    ip: IpAddr,
) -> Result<(StatusCode, Body)> {
    if !matches!(req.headers().get(hyper::header::CONTENT_TYPE), Some(ct) if ct == "application/x-www-form-urlencoded")
    {
        return Ok((StatusCode::BAD_REQUEST, Body::empty()));
    }
    let body = hyper::body::aggregate(req.body_mut()).await?;
    let form: HashMap<String, String> = serde_urlencoded::from_reader(body.reader())?;
    let token = form.get("token");
    if token == Some(&ctx.token) {
        ctx.service.lock().unwrap().push(ip);
        Ok((StatusCode::OK, "hello".into()))
    } else {
        warn!("未授权访问：{}", ip);
        Ok((StatusCode::FORBIDDEN, Body::empty()))
    }
}

fn get_remote_ip(req: &Request<Body>, remote_addr: &SocketAddr, allow_proxy: bool) -> IpAddr {
    let mut ip: Option<IpAddr> = None;
    if allow_proxy {
        if let Some(f) = req.headers().get("X-Forwarded-For") {
            if let Ok(f) = f.to_str() {
                let addr = if let Some((f, _)) = f.split_once(",") {
                    f.trim()
                } else {
                    f.trim()
                };
                if let Ok(addr) = addr.parse::<IpAddr>() {
                    ip = Some(addr);
                } else if let Ok(addr) = addr.parse::<SocketAddr>() {
                    ip = Some(addr.ip());
                }
            }
        }
    }
    if ip.is_none() {
        ip = Some(remote_addr.ip());
    }
    ip.unwrap()
}
