use crate::Result;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct BaiduLocationServiceConfig {
    pub ak: String,
    pub referrer: String,
}

pub struct BaiduLocationService {
    config: BaiduLocationServiceConfig,
    cache: Arc<Mutex<lru_time_cache::LruCache<std::net::IpAddr, String>>>,
}

impl BaiduLocationService {
    pub fn new(config: BaiduLocationServiceConfig) -> Self {
        BaiduLocationService {
            config,
            cache: Arc::new(Mutex::new(
                lru_time_cache::LruCache::with_expiry_duration_and_capacity(
                    std::time::Duration::from_secs(24 * 60 * 60),
                    100,
                ),
            )),
        }
    }

    pub async fn get(&self, ip: &std::net::IpAddr) -> Result<String> {
        if !ip.is_ipv4() || self.config.ak.is_empty() || self.config.referrer.is_empty() {
            return Err("参数错误".into());
        }
        if let Some(addr) = self.cache.lock().unwrap().get(ip) {
            debug!("从缓存获取 {} 的位置为 {}", ip, addr);
            return Ok(addr.into());
        }
        let resp = ureq::get("https://api.map.baidu.com/location/ip")
            .set("Referer", &self.config.referrer)
            .query("ak", &self.config.ak)
            .query("ip", &ip.to_string())
            .timeout(std::time::Duration::from_secs(15))
            .call()?;
        let data: serde_json::Value = serde_json::from_str(&resp.into_string()?)?;
        if data["status"].as_i64() == Some(0) {
            if let Some(addr) = data["content"]["address"].as_str() {
                debug!("联网获取 {} 的位置为 {}", ip, addr);
                self.cache.lock().unwrap().insert(ip.clone(), addr.into());
                return Ok(addr.into());
            }
        }
        return Err("解析结果失败".into());
    }
}
