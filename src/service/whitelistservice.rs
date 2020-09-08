use super::{BaiduLocationService, MessageService};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct WhiteListServiceConfig {
    pub nginx_conf: String,
    pub nginx_exe: String,
    pub remote_addr_var: String,
    pub result_var: String,
    pub timeout: Duration,
    pub loop_delay: Duration,
    pub ipv4_prefixlen: u8,
    pub ipv6_prefixlen: u8,
}

enum Message {
    Push(IpAddr),
    Terminate,
}

pub struct WhiteListService {
    handle: Option<thread::JoinHandle<()>>,
    sender: mpsc::Sender<Message>,
}

impl WhiteListService {
    pub fn new(
        config: WhiteListServiceConfig,
        msgsvc: Option<Arc<MessageService>>,
        locsvc: Option<Arc<Mutex<BaiduLocationService>>>,
    ) -> Self {
        let (s, r) = mpsc::channel::<Message>();
        let mut inner = WhiteListServiceImpl {
            config,
            list: HashMap::new(),
            last_list: Vec::new(),
            receiver: r,
            msgsvc,
            locsvc,
        };
        WhiteListService {
            handle: Some(thread::spawn(move || {
                // 启动时写出一个空配置
                inner.on_list_changed(&vec![]);
                inner.run();
            })),
            sender: s,
        }
    }

    pub fn push(&mut self, ip: IpAddr) {
        self.sender.send(Message::Push(ip)).unwrap();
    }

    pub fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.sender.send(Message::Terminate).unwrap();
            handle.join().unwrap();
        }
    }
}

impl Drop for WhiteListService {
    fn drop(&mut self) {
        self.stop();
    }
}

struct WhiteListServiceImpl {
    config: WhiteListServiceConfig,
    list: HashMap<IpAddr, Instant>,
    last_list: Vec<IpAddr>,
    receiver: mpsc::Receiver<Message>,
    msgsvc: Option<Arc<MessageService>>,
    locsvc: Option<Arc<Mutex<BaiduLocationService>>>,
}

impl WhiteListServiceImpl {
    fn run(&mut self) {
        loop {
            thread::sleep(self.config.loop_delay);
            while let Ok(msg) = self.receiver.try_recv() {
                match msg {
                    Message::Terminate => return,
                    Message::Push(ip) => self.push(ip),
                }
            }
            self.on_timer();
        }
    }

    fn push(&mut self, ip: IpAddr) {
        self.list.insert(ip, Instant::now() + self.config.timeout);
    }

    fn on_timer(&mut self) {
        self.list.retain(|_, t| &Instant::now() < t);
        let curlist: Vec<IpAddr> = self.list.keys().cloned().collect();
        let newip: Vec<IpAddr> = curlist
            .iter()
            .filter(|ip| !self.last_list.contains(ip))
            .cloned()
            .collect();
        let delip: Vec<IpAddr> = self
            .last_list
            .iter()
            .filter(|ip| !curlist.contains(ip))
            .cloned()
            .collect();
        if newip.len() > 0 || delip.len() > 0 {
            if newip.len() > 0 {
                let mut iplist = ipvec_to_strvec(&newip);
                debug!("新增 IP: \n\t{}", iplist.join("\n\t"));
                if let Some(msgsvc) = &self.msgsvc {
                    let mut rt = tokio::runtime::Runtime::new().unwrap();
                    if let Some(locsvc) = &self.locsvc {
                        iplist = newip
                            .iter()
                            .map(|ip| {
                                let mut ipstr = ip.to_string();
                                match rt.block_on(locsvc.lock().unwrap().get(ip)) {
                                    Ok(loc) => ipstr = format!("{}({})", ipstr, loc),
                                    Err(err) => error!("获取 {} 的位置失败: {}", ipstr, err),
                                }
                                ipstr
                            })
                            .collect();
                    }
                    if let Err(err) = rt.block_on(msgsvc.send(&iplist.join("; "))) {
                        error!("发送消息失败: {}", err);
                    }
                }
            }
            if delip.len() > 0 {
                debug!(
                    "删除 IP: \n\t{}",
                    delip
                        .iter()
                        .map(|ip| ip.to_string())
                        .collect::<Vec<String>>()
                        .join("\n\t")
                )
            }
            self.on_list_changed(&curlist);
            self.last_list = curlist;
        }
    }

    fn on_list_changed(&self, list: &Vec<IpAddr>) {
        let list = self.ipvec_with_prefix(list);
        if list.len() > 0 {
            info!("当前列表:\n\t{}", list.join("\n\t"));
        } else {
            info!("当前列表: 【空】");
        }

        let mut s = String::new();
        s.push_str(&format!(
            "geo ${} ${} {{\n",
            self.config.remote_addr_var, self.config.result_var
        ));
        s.push_str("default 0;\n");
        for i in list {
            s.push_str(&format!("{} 1;\n", i));
        }
        s.push_str("}\n");
        debug!("写出配置:\n{}", s);

        if let Err(err) = std::fs::write(&self.config.nginx_conf, s) {
            error!("写出配置文件失败: {}", err);
            return;
        }

        let cwd = std::path::Path::new(&self.config.nginx_exe)
            .parent()
            .unwrap();
        let p = std::process::Command::new(&self.config.nginx_exe)
            .arg("-t")
            .current_dir(cwd)
            .spawn();
        let status = p.and_then(|mut p| p.wait());
        if let Err(err) = status {
            error!("创建进程失败: {}", err);
            return;
        } else {
            let status = status.unwrap();
            if !status.success() {
                error!("新的配置文件测试失败: {}", status);
                return;
            }
        }

        let p = std::process::Command::new(&self.config.nginx_exe)
            .arg("-s")
            .arg("reload")
            .current_dir(cwd)
            .spawn();
        let status = p.and_then(|mut p| p.wait());
        if let Err(err) = status {
            error!("创建进程失败: {}", err);
            return;
        } else {
            let status = status.unwrap();
            if !status.success() {
                error!("刷新配置失败: {}", status);
                return;
            }
        }

        info!("已刷新配置");
    }

    fn ipvec_with_prefix(&self, v: &Vec<IpAddr>) -> Vec<String> {
        v.iter()
            .map(|ip| match ip {
                IpAddr::V4(ip) => ipv4_to_cidr(ip, self.config.ipv4_prefixlen),
                IpAddr::V6(ip) => ipv6_to_cidr(ip, self.config.ipv6_prefixlen),
            })
            .collect()
    }
}

fn ipvec_to_strvec(v: &Vec<IpAddr>) -> Vec<String> {
    v.iter().map(|ip| ip.to_string()).collect()
}

fn ipv4_to_cidr(ip: &Ipv4Addr, prefixlen: u8) -> String {
    if prefixlen == 0 || prefixlen == 32 {
        return ip.to_string();
    }
    let mut ipu32 = u32::from_be_bytes(ip.octets());
    let zerolen = 32 - prefixlen;
    ipu32 = ipu32 >> zerolen << zerolen;
    let ip4 = Ipv4Addr::from(ipu32);
    format!("{}/{}", ip4, prefixlen)
}

fn ipv6_to_cidr(ip: &Ipv6Addr, prefixlen: u8) -> String {
    if prefixlen == 0 || prefixlen == 128 {
        return ip.to_string();
    }
    let mut ipu128 = u128::from_be_bytes(ip.octets());
    let zerolen = 128 - prefixlen;
    ipu128 = ipu128 >> zerolen << zerolen;
    let ip6 = Ipv6Addr::from(ipu128);
    format!("{}/{}", ip6, prefixlen)
}
