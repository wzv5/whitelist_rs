use super::{BaiduLocationService, MessageService};
use std::{
    collections::HashMap,
    net::IpAddr,
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
                println!("新增 IP: \n\t{}", iplist.join("\n\t"));
                if let Some(msgsvc) = &self.msgsvc {
                    let mut rt = tokio::runtime::Runtime::new().unwrap();
                    if let Some(locsvc) = &self.locsvc {
                        iplist = newip
                            .iter()
                            .map(|ip| {
                                let mut ipstr = ip.to_string();
                                if let Ok(loc) = rt.block_on(locsvc.lock().unwrap().get(ip)) {
                                    ipstr = format!("{}({})", ipstr, loc);
                                }
                                ipstr
                            })
                            .collect();
                    }
                    let _ = rt.block_on(msgsvc.send(&iplist.join("; ")));
                }
            }
            if delip.len() > 0 {
                println!(
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
        if list.len() > 0 {
            println!("当前列表:\n\t{}", ipvec_to_strvec(list).join("\n\t"));
        } else {
            println!("当前列表: 【空】");
        }

        let mut s = String::new();
        s.push_str(&format!(
            "geo ${} ${} {{\n",
            self.config.remote_addr_var, self.config.result_var
        ));
        s.push_str("default 0;\n");
        for i in list {
            s.push_str(&format!("{} 1;\n", i.to_string()));
        }
        s.push_str("}\n");
        //println!("{}", s);

        if let Err(err) = std::fs::write(&self.config.nginx_conf, s) {
            println!("写出配置文件失败: {}", err);
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
            println!("创建进程失败: {}", err);
            return;
        } else {
            let status = status.unwrap();
            if !status.success() {
                println!("新的配置文件测试失败 [{}]", status);
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
            println!("创建进程失败: {}", err);
            return;
        } else {
            let status = status.unwrap();
            if !status.success() {
                println!("刷新配置失败 [{}]", status);
                return;
            }
        }

        println!("已刷新配置");
    }
}

fn ipvec_to_strvec(v: &Vec<IpAddr>) -> Vec<String> {
    v.iter().map(|ip| ip.to_string()).collect()
}
