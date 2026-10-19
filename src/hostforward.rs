//! Native Mac TCP forwarding. Control uses an owner-only Unix socket; no PID killing.
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    env,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Write},
    net::{Shutdown, SocketAddr, SocketAddrV4, TcpListener, TcpStream},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
            net::{UnixListener, UnixStream},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const TIMEOUT: Duration = Duration::from_secs(3);
const MAX_CONNECTIONS: usize = 128;
fn field<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn address(s: &str, listen: bool) -> Result<SocketAddrV4, String> {
    let a = s
        .parse::<SocketAddrV4>()
        .map_err(|_| "地址必须为 IPv4:端口（1..65535）")?;
    if a.port() == 0 || a.ip().is_unspecified() || a.ip().is_multicast() || a.ip().is_broadcast() {
        return Err("无效 IPv4 地址或端口".into());
    }
    if listen && !a.ip().is_loopback() {
        return Err("Mac 本地入口仅支持 127.0.0.0/8 回环地址；默认 127.0.0.1".into());
    }
    Ok(a)
}
fn config(v: &Value) -> Result<Value, String> {
    let listen = address(field(v, "listen"), true)?;
    let target = address(field(v, "target"), false)?;
    if listen == target {
        return Err("监听入口与实际目标相同，会形成转发循环".into());
    }
    Ok(
        json!({"listen":listen.to_string(),"target":target.to_string(),"vm":field(v,"vm"),"vm_id":field(v,"vm_id")}),
    )
}
fn state_dir() -> Result<PathBuf, String> {
    let dir = env::var_os("VNET_STUDIO_HOST_DIR")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".vnetstudio-host")))
        .ok_or("找不到 Mac 用户目录")?;
    match fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(format!("创建 Mac 规则目录失败：{e}")),
    }
    let m = fs::symlink_metadata(&dir).map_err(|e| e.to_string())?;
    if !m.is_dir() || m.uid() != unsafe { libc::getuid() } || m.mode() & 0o077 != 0 {
        return Err("Mac 规则目录必须属于当前用户且权限为 0700".into());
    }
    Ok(dir)
}
fn private_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
}
fn save(dir: &Path, configs: &BTreeMap<String, Value>) -> Result<(), String> {
    let tmp = dir.join("rules.tmp");
    let mut f = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| e.to_string())?;
    f.write_all(
        json!(configs.values().collect::<Vec<_>>())
            .to_string()
            .as_bytes(),
    )
    .and_then(|_| f.sync_all())
    .map_err(|e| format!("保存 Mac 规则失败：{e}"))?;
    fs::rename(tmp, dir.join("rules.json")).map_err(|e| format!("保存 Mac 规则失败：{e}"))
}
#[derive(Default)]
struct Telemetry {
    accepted: u64,
    rejected: u64,
    failed: u64,
    tx: u64,
    rx: u64,
    next: u64,
    active: BTreeMap<u64, (TcpStream, TcpStream)>,
    events: VecDeque<Value>,
    log_error: String,
    last_error: String,
}
fn event(t: &mut Telemetry, dir: &Path, key: &str, mut e: Value) {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    // Unix seconds are explicit UTC and avoid timezone-dependent formatting.
    e["time"] = json!(format!(
        "{}.{:03} UTC (Unix)",
        stamp.as_secs(),
        stamp.subsec_millis()
    ));
    if let Some(error) = e["error"].as_str().filter(|s| !s.is_empty()) {
        t.last_error = error.into();
    }
    let path = dir.join(format!("{}.events", key.replace(':', "-")));
    let result = (|| -> io::Result<()> {
        if fs::metadata(&path)
            .map(|m| m.len() > 1_048_576)
            .unwrap_or(false)
        {
            fs::rename(&path, path.with_extension("events.1"))?;
        }
        writeln!(private_file(&path)?, "{e}")
    })();
    if let Err(err) = result {
        t.log_error = err.to_string();
    }
    t.events.push_back(e);
    while t.events.len() > 200 {
        t.events.pop_front();
    }
}
struct Running {
    stop: Arc<AtomicBool>,
    telemetry: Arc<Mutex<Telemetry>>,
    accept: thread::JoinHandle<()>,
}
impl Running {
    fn stop(self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.accept.join(); // Listener is dropped before returning, so immediate restart is safe.
        for (a, b) in self.telemetry.lock().unwrap().active.values() {
            let _ = a.shutdown(Shutdown::Both);
            let _ = b.shutdown(Shutdown::Both);
        }
    }
}
// macOS may reject the first connect while nehelper refreshes an upgraded app's UUID.
// Retry only EHOSTUNREACH, at most three attempts within the original probe timeout.
fn retry_probe(mut attempt: impl FnMut(Duration)->io::Result<()>, delay:Duration)->io::Result<()> {
    let began=Instant::now();
    for index in 0..3 {
        let remaining=TIMEOUT.saturating_sub(began.elapsed());
        if remaining.is_zero(){return Err(io::Error::new(io::ErrorKind::TimedOut,"目标连接检查超时"));}
        match attempt(remaining) {
            Ok(())=>return Ok(()),
            Err(e) if cfg!(target_os="macos")&&e.raw_os_error()==Some(65)&&index<2&&began.elapsed()+delay<TIMEOUT=>thread::sleep(delay),
            Err(e)=>return Err(e),
        }
    }
    unreachable!()
}
fn probe(target: SocketAddrV4) -> Result<(), String> {
    retry_probe(|timeout|TcpStream::connect_timeout(&SocketAddr::V4(target),timeout).map(|s|{let _=s.shutdown(Shutdown::Both);}),Duration::from_millis(250))
        .map_err(|e|probe_error(target,&e))
}
fn probe_error(target:SocketAddrV4,error:&io::Error)->String {
    let hint=match error.kind(){
        io::ErrorKind::ConnectionRefused=>"目标拒绝了 TCP 连接。请确认目标 IP、服务端口，以及服务是否正在该地址监听；目标防火墙主动拒绝也可能导致此错误。",
        io::ErrorKind::TimedOut=>"连接目标超时。请检查目标服务、网络路由、VPN/TUN 和防火墙是否丢弃连接。",
        io::ErrorKind::PermissionDenied=>"连接被系统拒绝。请检查系统权限与防火墙策略。",
        _ if cfg!(target_os="macos")&&error.raw_os_error()==Some(65)=>"Mac 无法到达目标。请检查目标地址、Mac 路由及 VPN/TUN；macOS 本地网络权限也可能阻止连接，可在“系统设置 → 隐私与安全性 → 本地网络”中核对 VNet Studio。",
        _=>"请检查目标地址、服务监听状态、网络路由和防火墙。",
    };
    let scope=if target.ip().is_loopback(){" 当前目标是本地 Mac 的回环地址，不是虚拟机；访问虚拟机服务时请填写虚拟机网卡 IP，并保留实际服务端口。"}else{""};
    format!("Mac → 实际目标 {target} 连接失败：{error}。{hint}{scope}")
}
fn start(c: &Value, dir: &Path, telemetry: Arc<Mutex<Telemetry>>) -> Result<Running, String> {
    let key = field(c, "listen").to_string();
    let listener = TcpListener::bind(address(&key, true)?)
        .map_err(|e| format!("Mac 监听入口 {key} 绑定失败：{e}（检查端口占用 / 地址）"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let target = address(field(c, "target"), false)?;
    probe(target).map_err(|e|format!("{e} 本次未开启本地入口。"))?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let stats = telemetry.clone();
    let dir = dir.to_path_buf();
    event(
        &mut stats.lock().unwrap(),
        &dir,
        &key,
        json!({"event":"started","target":target.to_string()}),
    );
    let accept = thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((client, peer)) => {
                    // macOS accepted sockets inherit the listener's O_NONBLOCK flag.
                    if client.set_nonblocking(false).is_err() {
                        continue;
                    }
                    let mut t = stats.lock().unwrap();
                    t.accepted += 1;
                    if t.active.len() >= MAX_CONNECTIONS {
                        t.rejected += 1;
                        event(
                            &mut t,
                            &dir,
                            &key,
                            json!({"event":"rejected","peer":peer.to_string(),"target":target.to_string(),"error":"达到 128 个并发连接上限"}),
                        );
                        continue;
                    }
                    // Reserve a slot before connecting upstream; stop can cancel pending clients too.
                    let id = t.next;
                    t.next += 1;
                    let a = match client.try_clone() {
                        Ok(a) => a,
                        Err(_) => continue,
                    };
                    let b = match client.try_clone() {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    t.active.insert(id, (a, b));
                    drop(t);
                    let stats = stats.clone();
                    let dir = dir.clone();
                    let key = key.clone();
                    let flag = flag.clone();
                    thread::spawn(move || {
                        relay(client, peer, target, id, &stats, &dir, &key, &flag)
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20))
                }
                Err(e) => {
                    flag.store(true, Ordering::SeqCst);
                    event(
                        &mut stats.lock().unwrap(),
                        &dir,
                        &key,
                        json!({"event":"listen_error","error":e.to_string()}),
                    );
                    break;
                }
            }
        }
        event(
            &mut stats.lock().unwrap(),
            &dir,
            &key,
            json!({"event":"stopped"}),
        );
    });
    Ok(Running {
        stop,
        telemetry,
        accept,
    })
}
fn pump(
    reader: &mut TcpStream,
    writer: &mut TcpStream,
    stats: &Arc<Mutex<Telemetry>>,
    upload: bool,
) -> io::Result<u64> {
    let mut bytes = 0;
    let mut buf = [0u8; 32768];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => return Ok(bytes),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        writer.write_all(&buf[..n])?;
        bytes += n as u64;
        let mut t = stats.lock().unwrap();
        if upload {
            t.tx += n as u64;
        } else {
            t.rx += n as u64;
        }
    }
}
fn relay(
    mut client: TcpStream,
    peer: SocketAddr,
    target: SocketAddrV4,
    id: u64,
    stats: &Arc<Mutex<Telemetry>>,
    dir: &Path,
    key: &str,
    stop: &AtomicBool,
) {
    let began = Instant::now();
    let result = (|| -> io::Result<(u64, u64)> {
        let mut upstream = TcpStream::connect_timeout(&SocketAddr::V4(target), TIMEOUT)?;
        let mut cr = client.try_clone()?;
        let mut uw = upstream.try_clone()?;
        {
            let mut t = stats.lock().unwrap();
            if stop.load(Ordering::SeqCst) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "规则已停止"));
            }
            t.active
                .insert(id, (client.try_clone()?, upstream.try_clone()?));
            event(
                &mut t,
                dir,
                key,
                json!({"event":"connected","peer":peer.to_string(),"target":target.to_string()}),
            );
        }
        // A FIN only closes that direction. Protocols with request half-close still receive a response.
        thread::scope(|s| {
            let up = s.spawn(move || {
                let n = pump(&mut cr, &mut uw, stats, true);
                let _ = uw.shutdown(Shutdown::Write);
                if n.is_err() {
                    let _ = cr.shutdown(Shutdown::Both);
                    let _ = uw.shutdown(Shutdown::Both);
                }
                n
            });
            let down = pump(&mut upstream, &mut client, stats, false);
            let _ = client.shutdown(Shutdown::Write);
            if down.is_err() {
                let _ = client.shutdown(Shutdown::Both);
                let _ = upstream.shutdown(Shutdown::Both);
            }
            let tx = up.join().map_err(|_| io::Error::other("发送线程退出"))??;
            Ok((tx, down?))
        })
    })();
    let mut t = stats.lock().unwrap();
    t.active.remove(&id);
    let (tx, rx, error) = match result {
        Ok((tx, rx)) => (tx, rx, String::new()),
        Err(e) => {
            if !stop.load(Ordering::SeqCst) {
                t.failed += 1;
            }
            (0, 0, format!("Mac → {target}：{e}"))
        }
    };
    event(
        &mut t,
        dir,
        key,
        json!({"event":if error.is_empty(){"closed"}else{"error"},"peer":peer.to_string(),"target":target.to_string(),"tx":tx,"rx":rx,"duration_ms":began.elapsed().as_millis() as u64,"error":error}),
    );
}
struct Manager {
    dir: PathBuf,
    configs: BTreeMap<String, Value>,
    running: BTreeMap<String, Running>,
    stats: BTreeMap<String, Arc<Mutex<Telemetry>>>,
}
impl Manager {
    fn load(dir: PathBuf) -> Result<Self, String> {
        let rows = match fs::read(dir.join("rules.json")) {
            Ok(b) => serde_json::from_slice::<Vec<Value>>(&b)
                .map_err(|e| format!("Mac 规则文件损坏：{e}"))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => vec![],
            Err(e) => return Err(e.to_string()),
        };
        let mut configs: BTreeMap<String, Value> = BTreeMap::new();
        for row in rows {
            let c = config(&row)?;
            configs.insert(field(&c, "listen").into(), c);
        }
        let mut stats = BTreeMap::new();
        for key in configs.keys() {
            let mut t = Telemetry::default();
            let path = dir.join(format!("{}.events", key.replace(':', "-")));
            for p in [path.with_extension("events.1"), path] {
                if let Ok(f) = File::open(p) {
                    for line in BufReader::new(f).lines().map_while(Result::ok) {
                        if let Ok(e) = serde_json::from_str::<Value>(&line) {
                            t.events.push_back(e);
                            while t.events.len() > 200 {
                                t.events.pop_front();
                            }
                        }
                    }
                }
            }
            stats.insert(key.clone(), Arc::new(Mutex::new(t)));
        }
        Ok(Self {
            dir,
            configs,
            running: BTreeMap::new(),
            stats,
        })
    }
    fn list(&self) -> Value {
        let rows=self.configs.iter().map(|(key,c)|{
            let telemetry=self.stats.get(key).cloned().unwrap_or_default();let t=telemetry.lock().unwrap();
            let addr=address(key,true).unwrap();let running=self.running.get(key).map(|r|!r.stop.load(Ordering::SeqCst)).unwrap_or(false);
            json!({"kind":"forward","location":"host","ip":addr.ip().to_string(),"port":addr.port(),"target":c["target"],"vm":c["vm"],"vm_id":c["vm_id"],"allow":"127.0.0.0/8","managed":true,"running":running,"status":if running{"running"}else{"stopped"},"telemetry":"connections","stats":{"active":t.active.len(),"accepted":t.accepted,"failed":t.failed,"rejected":t.rejected,"tx":t.tx,"rx":t.rx},"events":t.events,"log_error":t.log_error,"last_error":t.last_error,"url":format!("http://{key}")})
        }).collect::<Vec<_>>();
        json!({"services":rows,"note":"Mac 本地转发；关闭 GUI 后继续运行。Mac 重启后保留配置，需手动开启。目标连接使用 Mac 当前路由，仍可能受 VPN/TUN 影响。"})
    }
    fn handle(&mut self, request: Value) -> Result<Value, String> {
        let op = field(&request, "op");
        if op == "list" {
            return Ok(self.list());
        }
        if op == "batch" {
            let rows = request["rules"].as_array().ok_or("缺少规则数组")?;
            if rows.is_empty() || rows.len() > 128 {
                return Err("每批需要 1 至 128 条规则".into());
            }
            let mut seen = std::collections::HashSet::new();
            let configs = rows.iter().map(config).collect::<Result<Vec<_>, _>>()?;
            for c in &configs {
                if !seen.insert(field(c, "listen")) {
                    return Err("批次内监听入口重复".into());
                }
            }
            let mut failed = false;
            let mut results = vec![];
            for (i, c) in configs.into_iter().enumerate() {
                if failed {
                    results.push(json!({"row":i+1,"state":"未执行"}));
                    continue;
                }
                let result = self.handle(json!({"op":"add","config":c}));
                failed = result.is_err();
                results.push(json!({"row":i+1,"state":if failed{"失败"}else{"已开启"},"detail":result.unwrap_or_else(|e|json!(e))}));
            }
            return Ok(json!({"ok":!failed,"results":results}));
        }
        let key = if op == "add" {
            config(&request["config"])?["listen"]
                .as_str()
                .unwrap()
                .to_string()
        } else {
            address(field(&request, "listen"), true)?.to_string()
        };
        if matches!(op, "add" | "start") {
            if self
                .running
                .get(&key)
                .map(|r| r.stop.load(Ordering::SeqCst))
                .unwrap_or(false)
            {
                if let Some(r) = self.running.remove(&key) {
                    r.stop();
                }
            }
            if self.running.contains_key(&key) {
                return Err(format!("Mac 入口 {key} 已运行；请先停止再修改"));
            }
            let c = if op == "add" {
                config(&request["config"])?
            } else {
                self.configs
                    .get(&key)
                    .cloned()
                    .ok_or("没有本工具保存的 Mac 规则")?
            };
            if !self.configs.contains_key(&key) && self.configs.len() >= 128 {
                return Err("Mac 本地规则最多 128 条，请先删除不再使用的规则".into());
            }
            let mut at = field(&c, "target");
            let mut seen = std::collections::HashSet::new();
            while let Some(next) = self.configs.get(at) {
                if at == key || !seen.insert(at) {
                    return Err("实际目标经过已有 Mac 规则后形成转发循环".into());
                }
                at = field(next, "target");
            }
            if at == key {
                return Err("实际目标经过已有 Mac 规则后形成转发循环".into());
            }
            let stats = self.stats.entry(key.clone()).or_default().clone();
            let running = match start(&c, &self.dir, stats.clone()) {
                Ok(r) => r,
                Err(e) => {
                    event(
                        &mut stats.lock().unwrap(),
                        &self.dir,
                        &key,
                        json!({"event":"start_error","target":c["target"],"error":e}),
                    );
                    return Err(e);
                }
            };
            let mut next = self.configs.clone();
            next.insert(key.clone(), c);
            if let Err(e) = save(&self.dir, &next) {
                running.stop();
                return Err(e);
            }
            self.configs = next;
            self.running.insert(key.clone(), running);
            return Ok(
                json!({"message":"Mac 监听成功，实际目标 TCP 连接检查通过；请打开网址验证页面。","url":format!("http://{key}")}),
            );
        }
        let c = self
            .configs
            .get(&key)
            .cloned()
            .ok_or("没有本工具保存的 Mac 规则")?;
        match op {
            "stop" | "delete" => {
                if op == "delete" {
                    let mut next = self.configs.clone();
                    next.remove(&key);
                    save(&self.dir, &next)?;
                    self.configs = next;
                }
                if let Some(r) = self.running.remove(&key) {
                    r.stop();
                }
                if op == "delete" {
                    self.stats.remove(&key);
                }
                Ok(
                    json!({"message":if op=="stop"{"Mac 转发已停止，配置保留"}else{"Mac 转发已停止并删除，磁盘连接日志保留"}}),
                )
            }
            "check" => {
                let target = address(field(&c, "target"), false)?;
                let result = probe(target);
                let route = Command::new("/sbin/route")
                    .args(["-n", "get", &target.ip().to_string()])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                    .unwrap_or_default();
                Ok(
                    json!({"listen":key,"running":self.running.contains_key(&key),"target":target.to_string(),"target_tcp_ok":result.is_ok(),"error":result.err(),"route":route,"note":"TCP 成功不代表浏览器页面、HTTP/TLS 或应用请求成功；浏览器系统代理与 Mac→目标的 VPN/TUN 路由需分别核对。"}),
                )
            }
            _ => Err("未知 Mac 转发操作".into()),
        }
    }
}
fn read_json(stream: &mut UnixStream, limit: u64) -> Result<Value, String> {
    let mut line = String::new();
    BufReader::new(stream.take(limit))
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&line).map_err(|e| format!("Mac 控制响应无效：{e}"))
}
fn build_identity() -> String {
    env::current_exe().ok().and_then(|p|fs::metadata(p).ok()).map(|m| {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}:{}:{}",m.ino(),m.len(),m.mtime(),m.mtime_nsec())
    }).unwrap_or_default()
}
pub fn restart_idle() -> Result<Value,String> {
    request(json!({"op":"shutdown-idle"})).map_err(|e|format!("无法重启后台：{e}。旧版后台需在停止全部转发后退出，再重新打开应用。"))?;
    let socket=state_dir()?.join("control.sock");let began=Instant::now();
    while socket.exists() {if began.elapsed()>Duration::from_secs(4){return Err("旧后台尚未退出，请刷新状态后重试".into());}thread::sleep(Duration::from_millis(40));}
    request(json!({"op":"list"}))
}
pub fn serve() -> Result<(), String> {
    let dir = state_dir()?;
    let lock = private_file(&dir.join("daemon.lock")).map_err(|e| e.to_string())?;
    // The kernel releases flock on exit/crash. Only its owner may recover a stale socket.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Ok(());
    }
    let socket = dir.join("control.sock");
    if socket.exists() {
        fs::remove_file(&socket).map_err(|e| e.to_string())?;
    }
    let mut manager = Manager::load(dir)?;
    let listener = UnixListener::bind(&socket).map_err(|e| format!("Mac 控制通道绑定失败：{e}"))?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    let build=build_identity();
    for stream in listener.incoming() {
        let mut stream = stream.map_err(|e| e.to_string())?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
        let incoming=read_json(&mut stream,1024*1024);
        let shutdown=incoming.as_ref().map(|r|field(r,"op")=="shutdown-idle").unwrap_or(false);
        let result=incoming.and_then(|r|if shutdown {
            if manager.running.values().any(|r|!r.stop.load(Ordering::SeqCst)){Err("仍有运行中的 Mac 转发，请先逐条停止；重启不会中断现有服务".into())}else{Ok(json!({"message":"空闲后台已退出，配置保留"}))}
        }else{manager.handle(r)});
        let exit=shutdown&&result.is_ok();
        let reply=match result {Ok(mut v)=>{if v.get("services").is_some(){v["backend_version"]=json!(env!("CARGO_PKG_VERSION"));v["backend_build"]=json!(build);}json!({"ok":true,"data":v})},Err(e)=>json!({"ok":false,"error":e})};
        let _ = writeln!(stream,"{reply}");
        if exit {let _=fs::remove_file(&socket);return Ok(());}
    }
    Ok(())
}
pub fn request(request: Value) -> Result<Value, String> {
    let dir = state_dir()?;
    let socket = dir.join("control.sock");
    let mut stream = match UnixStream::connect(&socket) {
        Ok(s) => s,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            let log = private_file(&dir.join("daemon.log")).map_err(|e| e.to_string())?;
            let mut command = Command::new(env::current_exe().map_err(|e| e.to_string())?);
            command
                .args(["host-forward", "serve"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log);
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let mut child = command
                .spawn()
                .map_err(|e| format!("启动 Mac 转发后端失败：{e}"))?;
            // Reap a competing startup process or this daemon when it eventually exits.
            thread::spawn(move || {
                let _ = child.wait();
            });
            let began = Instant::now();
            loop {
                match UnixStream::connect(&socket) {
                    Ok(s) => break s,
                    Err(e) => {
                        if began.elapsed() > Duration::from_secs(4) {
                            return Err(format!(
                                "Mac 转发后端未就绪：{e}；查看 {}",
                                dir.join("daemon.log").display()
                            ));
                        }
                        thread::sleep(Duration::from_millis(40));
                    }
                }
            }
        }
        Err(e) => return Err(format!("Mac 转发控制通道不可达，不能判断运行状态：{e}")),
    };
    // A batch can probe 128 destinations, each with a bounded timeout.
    stream
        .set_read_timeout(Some(Duration::from_secs(400)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    writeln!(stream, "{request}").map_err(|e| e.to_string())?;
    let response = read_json(&mut stream, 64 * 1024 * 1024)?;
    if response["ok"] != true {
        return Err(field(&response, "error").into());
    }
    let mut data=response["data"].clone();
    if data.get("services").is_some(){data["client_build"]=json!(build_identity());}
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probe_errors_only_suggest_relevant_remedies(){
        let local="127.0.0.1:8787".parse().unwrap();
        let remote="10.211.55.18:8787".parse().unwrap();
        let refused=probe_error(local,&io::Error::new(io::ErrorKind::ConnectionRefused,"refused"));
        assert!(refused.contains("目标拒绝")&&refused.contains("本地 Mac 的回环地址"));
        assert!(!refused.contains("本地网络权限")&&!refused.contains("VPN/TUN")&&!refused.contains("65"));
        let timeout=probe_error(remote,&io::Error::new(io::ErrorKind::TimedOut,"timeout"));
        assert!(timeout.contains("超时")&&!timeout.contains("回环")&&!timeout.contains("本地网络权限"));
        #[cfg(target_os="macos")]{
            let route=probe_error(remote,&io::Error::from_raw_os_error(65));
            assert!(route.contains("本地网络权限")&&route.contains("Mac 路由"));
        }
    }
    #[test]
    #[cfg(target_os="macos")]
    fn local_network_cache_retry_is_bounded_and_specific() {
        let mut calls=0;
        assert!(retry_probe(|_|{calls+=1;if calls<3{Err(io::Error::from_raw_os_error(65))}else{Ok(())}},Duration::ZERO).is_ok());
        assert_eq!(calls,3);
        calls=0;
        assert!(retry_probe(|_|{calls+=1;Err(io::Error::from_raw_os_error(65))},Duration::ZERO).is_err());
        assert_eq!(calls,3);
        calls=0;
        assert!(retry_probe(|_|{calls+=1;Err(io::Error::new(io::ErrorKind::ConnectionRefused,"refused"))},Duration::ZERO).is_err());
        assert_eq!(calls,1);
    }
    #[test]
    fn validates_local_scope_and_loops() {
        for ip in ["0.0.0.0:1234", "10.211.55.2:1234", "127.0.0.1:0"] {
            assert!(address(ip, true).is_err());
        }
        assert!(config(&json!({"listen":"127.0.0.1:1234","target":"127.0.0.1:1234"})).is_err());
        assert!(config(&json!({"listen":"127.0.0.1:18787","target":"10.211.55.18:8787"})).is_ok());
    }
    #[test]
    fn forwards_half_closed_request_and_stop_releases_port() {
        let dir = env::temp_dir().join(format!("vnet-host-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
        let target = upstream.local_addr().unwrap();
        let server = thread::spawn(move || {
            drop(upstream.accept().unwrap()); // Start preflight.
            let (mut s, _) = upstream.accept().unwrap();
            let mut b = vec![];
            s.read_to_end(&mut b).unwrap();
            assert_eq!(b, b"request");
            s.write_all(b"response").unwrap();
        });
        let free = TcpListener::bind("127.0.0.1:0").unwrap();
        let listen = free.local_addr().unwrap();
        drop(free);
        let t = Arc::new(Mutex::new(Telemetry::default()));
        let c = json!({"listen":listen.to_string(),"target":target.to_string()});
        let r = start(&c, &dir, t.clone()).unwrap();
        let mut client = TcpStream::connect(listen).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        client.write_all(b"request").unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        let mut response = vec![];
        client.read_to_end(&mut response).unwrap();
        assert_eq!(response, b"response");
        server.join().unwrap();
        r.stop();
        assert!(TcpListener::bind(listen).is_ok());
        let t = t.lock().unwrap();
        assert_eq!(t.tx, 7);
        assert_eq!(t.rx, 8);
        assert_eq!(t.active.len(), 0);
        assert!(t.events.iter().any(|e| e["event"] == "closed"));
        drop(t);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn manager_preserves_independent_rules_and_closes_active_clients() {
        let dir = env::temp_dir().join(format!("vnet-manager-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
        let target = upstream.local_addr().unwrap();
        let free = || {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().to_string()
        };
        let a = free();
        let b = free();
        let mut m = Manager::load(dir.clone()).unwrap();
        for key in [&a, &b] {
            m.handle(
                json!({"op":"add","config":{"listen":key,"target":target.to_string(),"vm":"Kali"}}),
            )
            .unwrap();
        }
        assert_eq!(m.list()["services"].as_array().unwrap().len(), 2);
        let mut client = TcpStream::connect(&a).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let began = Instant::now();
        while m.stats[&a].lock().unwrap().active.is_empty() {
            assert!(began.elapsed() < Duration::from_secs(2));
            thread::sleep(Duration::from_millis(10));
        }
        m.handle(json!({"op":"stop","listen":a})).unwrap();
        let mut byte = [0];
        assert!(client.read(&mut byte).map(|n| n == 0).unwrap_or(true));
        assert!(TcpListener::bind(&a).is_ok());
        assert!(TcpStream::connect(&b).is_ok());
        m.handle(json!({"op":"start","listen":a})).unwrap();
        m.handle(json!({"op":"delete","listen":a})).unwrap();
        m.handle(json!({"op":"stop","listen":b})).unwrap();
        let restored = Manager::load(dir.clone()).unwrap();
        let rows = restored.list();
        assert_eq!(rows["services"].as_array().unwrap().len(), 1);
        assert_eq!(rows["services"][0]["running"], false);
        assert!(!rows["services"][0]["events"].as_array().unwrap().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn failed_start_leaves_no_listener_or_saved_rule_and_batch_prevalidates() {
        let dir = env::temp_dir().join(format!("vnet-failed-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
        let key = occupied.local_addr().unwrap().to_string();
        let mut m = Manager::load(dir.clone()).unwrap();
        let cfg = json!({"listen":key,"target":"127.0.0.1:1"});
        assert!(m
            .handle(json!({"op":"add","config":cfg}))
            .unwrap_err()
            .contains("绑定失败"));
        drop(occupied);
        assert!(m
            .handle(json!({"op":"add","config":cfg}))
            .unwrap_err()
            .contains("实际目标"));
        assert!(TcpListener::bind(&key).is_ok());
        assert!(m.configs.is_empty());
        assert!(m.handle(json!({"op":"batch","rules":[cfg,{"listen":"0.0.0.0:18787","target":"127.0.0.1:1"}]})).is_err());
        assert!(m.configs.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }
}
