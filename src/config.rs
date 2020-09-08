extern crate serde;

use serde::Deserialize;
use std::{path, env};

fn find_config() -> Option<path::PathBuf> {
    // 1. 使用环境变量指定的配置文件
    if let Ok(envcfg) = env::var("APP_CONFIG") {
        let p = path::Path::new(&envcfg);
        if path::Path::is_file(p) {
            return Some(p.into());
        }
    }
    // 2. 当前工作目录下的 config.json
    if let Ok(cwd) = env::current_dir() {
        let p = cwd.join("config.json");
        if path::Path::is_file(&p) {
            return Some(p);
        }
    }
    // 3. exe 目录下的 config.json
    if let Ok(exe) = env::current_exe() {
        if let Some(cwd) = exe.parent() {
            let p = cwd.join("config.json");
            if path::Path::is_file(&p) {
                return Some(p);
            }
        }
    }
    None
}

pub(crate) fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    let p = find_config();
    if p.is_none() {
        return Err("找不到配置文件".into());
    }
    let p = p.unwrap();
    info!("配置文件: {}", p.display());
    let cfgfile = std::fs::File::open(p)?;
    let data: Config = serde_json::from_reader(cfgfile)?;
    Ok(data)
}

#[derive(Deserialize)]
pub(crate) struct Config {
    pub listen: ListenConfig,

    pub whitelist: WhiteListConfig,

    #[serde(default)]
    pub message: MessageConfig,

    #[serde(default)]
    pub baidu_location: BaiduLocationConfig,
}

#[derive(Deserialize)]
pub(crate) struct ListenConfig {
    pub urls: Vec<String>,
    pub path: String,

    #[serde(default = "default_allow_proxy")]
    pub allow_proxy: bool
}

fn default_allow_proxy() -> bool {
    true
}

#[derive(Deserialize)]
pub(crate) struct WhiteListConfig {
    pub token: String,
    pub nginx_conf: String,
    pub nginx_exe: String,

    #[serde(default = "default_remote_addr_var")]
    pub remote_addr_var: String,

    #[serde(default = "default_result_var")]
    pub result_var: String,

    #[serde(default = "default_timeout")]
    pub timeout: u32,

    #[serde(default = "default_loop_delay")]
    pub loop_delay: u32,

    #[serde(default)]
    pub ipv4_prefixlen: u8,

    #[serde(default)]
    pub ipv6_prefixlen: u8
}

fn default_remote_addr_var() -> String {
    "remote_addr".into()
}

fn default_result_var() -> String {
    "ip_whitelist".into()
}

fn default_timeout() -> u32 {
    3600
}

fn default_loop_delay() -> u32 {
    15
}

#[derive(Deserialize)]
pub(crate) struct MessageConfig {
    pub bark: String,
}

impl Default for MessageConfig {
    fn default() -> Self {
        MessageConfig {
            bark: String::new(),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct BaiduLocationConfig {
    pub ak: String,
    pub referrer: String,
}

impl Default for BaiduLocationConfig {
    fn default() -> Self {
        BaiduLocationConfig {
            ak: String::new(),
            referrer: String::new(),
        }
    }
}
