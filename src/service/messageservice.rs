#[derive(Clone)]
pub struct MessageServiceConfig {
    pub bark: String,
}

pub struct MessageService {
    config: MessageServiceConfig,
}

impl MessageService {
    pub fn new(config: MessageServiceConfig) -> Self {
        MessageService { config }
    }

    pub async fn send(&self, msg: &str) -> Result<(), Box<dyn std::error::Error>> {
        if msg.is_empty() || self.config.bark.is_empty() {
            return Err("参数错误".into());
        }
        debug!("发送消息: {}", msg);
        let url = format!(
            "{}/{}",
            self.config.bark,
            url::form_urlencoded::byte_serialize(msg.as_bytes()).collect::<String>()
        );
        let resp = ureq::get(&url)
            .timeout(std::time::Duration::from_secs(15))
            .call()?;
        let status = resp.status();
        if status != 200 {
            return Err(resp.status_text().into());
        }
        Ok(())
    }
}
