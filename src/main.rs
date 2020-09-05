extern crate actix_rt;
extern crate actix_web;
extern crate env_logger;
extern crate tokio;
#[macro_use]
extern crate log;

mod config;
mod service;

use actix_web::{middleware, web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use service::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

struct MyAppData {
    service: Mutex<WhiteListService>,
    token: String,
}

fn main() {
    env_logger::init();

    let cfg = config::load_config().unwrap();
    let listcfg = WhiteListServiceConfig {
        nginx_conf: cfg.whitelist.nginx_conf,
        nginx_exe: cfg.whitelist.nginx_exe,
        remote_addr_var: cfg.whitelist.remote_addr_var,
        result_var: cfg.whitelist.result_var,
        timeout: Duration::from_secs(cfg.whitelist.timeout.into()),
        loop_delay: Duration::from_secs(cfg.whitelist.loop_delay.into()),
    };
    let mut msgsvc: Option<Arc<MessageService>> = None;
    let mut locsvc: Option<Arc<Mutex<BaiduLocationService>>> = None;
    if !cfg.message.bark.is_empty() {
        msgsvc = Some(Arc::new(MessageService::new(MessageServiceConfig {
            bark: cfg.message.bark,
        })));
    }
    if !cfg.baidu_location.ak.is_empty() && !cfg.baidu_location.referrer.is_empty() {
        locsvc = Some(Arc::new(Mutex::new(BaiduLocationService::new(
            BaiduLocationServiceConfig {
                ak: cfg.baidu_location.ak,
                referrer: cfg.baidu_location.referrer,
            },
        ))))
    }
    let appdata = web::Data::new(MyAppData {
        service: Mutex::new(WhiteListService::new(listcfg, msgsvc, locsvc)),
        token: cfg.whitelist.token,
    });
    let listen_path = cfg.listen.path;

    let mut s = HttpServer::new(move || {
        App::new().wrap(middleware::Logger::default()).service(
            web::resource(&listen_path)
                .app_data(appdata.clone())
                .route(web::get().to(get))
                .route(web::post().to(post)),
        )
    })
    .workers(1);
    for i in cfg.listen.urls.iter() {
        s = s.bind(i).unwrap();
    }

    let _ = actix_rt::System::new("").block_on(s.run());
}

async fn get(req: HttpRequest) -> impl Responder {
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="zh">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <meta http-equiv="X-UA-Compatible" content="ie=edge">
    <title>登录</title>
</head>
<body>
    <form action="{}" method="POST">
        <label for="token">token: </label>
        <input name="token" id="token"/>
        <button type="submit">提交</button>
    </form>
</body>
</html>
"#,
        req.path()
    );
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html)
}

async fn post(
    req: HttpRequest,
    data: web::Data<MyAppData>,
    form: web::Form<HashMap<String, String>>,
) -> HttpResponse {
    if let Some(addr) = req.peer_addr() {
        if form.get("token") == Some(&data.token) {
            data.service.lock().unwrap().push(addr.ip());
            return "hello".into();
        } else {
            warn!("未授权访问：{}", addr.ip());
        }
    }
    HttpResponse::Forbidden().into()
}
