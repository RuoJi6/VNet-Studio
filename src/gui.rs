use eframe::egui::{self, Color32, RichText};
use std::{path::PathBuf, process::Command, sync::mpsc, time::Duration};

mod guest;
mod hostnet;
mod topology;
include!("topology_ui.rs");
include!("network_ui.rs");
include!("services_ui.rs");
include!("routes_ui.rs");
include!("workspace_ui.rs");

const BLUE: Color32 = Color32::from_rgb(44, 94, 230);
#[derive(PartialEq, Clone, Copy)]
enum Page {
    Overview,
    Host,
    Topology,
    Forward,
    Socks,
    Vpn,
    Containers,
    Services,
    Routes,
}
struct Reply {
    text: String,
    ok: bool,
    purpose: String,
    args: Vec<String>,
}
struct App {
    page: Page,
    forward_workspace: ForwardWorkspace,
    topology: topology::View,
    topology_services: TopologyServices,
    services: ServicesView,
    local_services: ServicesView,
    local_forward: bool,
    local_ip: String,
    local_port: String,
    local_target: String,
    local_batch_mode: bool,
    local_batch_text: String,
    routes: RoutesView,
    topology_tool: Option<Page>,
    selection: String,
    address_rows: Vec<serde_json::Value>,
    ip_search: String,
    host: serde_json::Value,
    net_device: String,
    net_mode: String,
    bridge_iface: String,
    usb_device: String,
    usb_search: String,
    pending_host: Option<serde_json::Value>,
    vm: String,
    vms: Vec<guest::Guest>,
    containers: Vec<serde_json::Value>,
    listen_ip: String,
    port: String,
    target: String,
    allow: String,
    socks_port: String,
    gost: String,
    vpn: String,
    output: String,
    log: Vec<String>,
    pending: Option<Vec<String>>,
    rx: Option<mpsc::Receiver<Reply>>,
    request_page: Page,
    message: String,
    good: bool,
}
fn state_label(s: &str) -> &str {
    match s {
        "running" => "运行中",
        "suspended" => "已挂起",
        "stopped" => "已停止",
        "paused" => "已暂停",
        "starting" => "启动中",
        "stopping" => "停止中",
        "suspending" => "挂起中",
        "resuming" => "恢复中",
        "unknown" | "" => "状态未知",
        _ => s,
    }
}
fn vm_state_color(state: &str) -> Color32 {
    match state {
        "running" => Color32::from_rgb(24, 120, 84),
        "suspended" | "paused" => Color32::from_rgb(160, 100, 20),
        "starting" | "stopping" | "suspending" | "resuming" => BLUE,
        _ => Color32::from_rgb(110, 117, 128),
    }
}
fn vm_choice_text(g: &guest::Guest) -> RichText {
    RichText::new(format!(
        "● {} · {} · {}",
        state_label(&g.state),
        g.name,
        g.os
    ))
    .color(vm_state_color(&g.state))
}
fn cli_path() -> PathBuf {
    std::env::current_exe()
        .unwrap_or_default()
        .with_file_name("vnetctl-cli")
}
fn prl_path() -> String {
    for path in [
        "/usr/local/bin/prlctl",
        "/Applications/Parallels Desktop.app/Contents/MacOS/prlctl",
    ] {
        if std::path::Path::new(path).exists() {
            return path.into();
        }
    }
    "prlctl".into()
}
fn execute(args: &[String], purpose: &str) -> Reply {
    if purpose.starts_with("host-") {
        let result = hostnet::handle(&prl_path(), purpose, args);
        return Reply {
            ok: result.is_ok(),
            text: result.unwrap_or_else(|e| e),
            purpose: purpose.into(),
            args: args.to_vec(),
        };
    }
    let mut cmd = if purpose == "vms" {
        let mut c = Command::new(prl_path());
        c.args(["list", "-a", "-i", "-j"]);
        c
    } else {
        let mut c = Command::new(cli_path());
        c.args(args).args(["--prlctl", &prl_path()]);
        c
    };
    match cmd.output() {
        Ok(o) => Reply {
            text: format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            ),
            ok: o.status.success(),
            purpose: purpose.into(),
            args: args.to_vec(),
        },
        Err(e) => Reply {
            text: format!("无法启动管理程序：{e}"),
            ok: false,
            purpose: purpose.into(),
            args: args.to_vec(),
        },
    }
}
impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        let mut fonts = egui::FontDefinitions::default();
        for path in [
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/System/Library/Fonts/PingFang.ttc",
        ] {
            if let Ok(data) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert("Chinese".into(), egui::FontData::from_owned(data).into());
                for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    fonts
                        .families
                        .entry(family)
                        .or_default()
                        .push("Chinese".into());
                }
                break;
            }
        }
        cc.egui_ctx.set_fonts(fonts);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(10., 10.);
        style.spacing.button_padding = egui::vec2(12., 7.);
        style.spacing.interact_size.y=32.;
        style.visuals.override_text_color=Some(INK);
        style.visuals.panel_fill=Color32::WHITE;
        style.visuals.window_fill=Color32::WHITE;
        style.visuals.extreme_bg_color=Color32::WHITE;
        style.visuals.faint_bg_color=Color32::from_rgb(248,249,250);
        style.visuals.selection.bg_fill=Color32::from_rgb(229,233,239);
        style.visuals.selection.stroke=egui::Stroke::new(1.,INK);
        for widget in [&mut style.visuals.widgets.inactive,&mut style.visuals.widgets.noninteractive] {widget.bg_fill=Color32::WHITE;widget.weak_bg_fill=Color32::WHITE;widget.bg_stroke=egui::Stroke::new(1.,LINE);widget.corner_radius=egui::CornerRadius::same(6);}
        style.visuals.widgets.hovered.bg_fill=Color32::from_rgb(243,244,246);
        style.visuals.widgets.hovered.weak_bg_fill=Color32::from_rgb(243,244,246);
        style.visuals.widgets.hovered.bg_stroke=egui::Stroke::new(1.,Color32::from_rgb(195,199,206));
        style.text_styles.insert(egui::TextStyle::Body,egui::FontId::proportional(14.));
        style.text_styles.insert(egui::TextStyle::Button,egui::FontId::proportional(13.));
        style.text_styles.insert(egui::TextStyle::Small,egui::FontId::proportional(12.));
        style.text_styles.insert(egui::TextStyle::Heading,egui::FontId::proportional(21.));
        cc.egui_ctx.set_style(style);
        let mut a = Self {
            page: Page::Overview,
            forward_workspace: ForwardWorkspace::default(),
            topology: topology::View::default(),
            topology_services: TopologyServices::default(),
            services: ServicesView::default(),
            local_services: ServicesView {auto:true, ..Default::default()},
            local_forward: true,
            local_ip: "127.0.0.1".into(),
            local_port: String::new(),
            local_target: String::new(),
            local_batch_mode: false,
            local_batch_text: String::new(),
            routes: RoutesView::default(),
            topology_tool: None,
            selection: String::new(),
            address_rows: vec![],
            ip_search: String::new(),
            host: serde_json::Value::Null,
            net_device: String::new(),
            net_mode: "shared".into(),
            bridge_iface: String::new(),
            usb_device: String::new(),
            usb_search: String::new(),
            pending_host: None,
            vm: String::new(),
            vms: vec![],
            containers: vec![],
            listen_ip: String::new(),
            port: String::new(),
            target: String::new(),
            allow: "10.211.55.2/32".into(),
            socks_port: "1080".into(),
            gost: "gost.exe".into(),
            vpn: String::new(),
            output: "选择虚拟机查看状态；支持 Windows、Linux 和 macOS 来宾。".into(),
            log: vec![],
            pending: None,
            rx: None,
            request_page: Page::Overview,
            message: "就绪 · 不会自动修改现有网络".into(),
            good: true,
        };
        a.launch(vec![], "vms");
        a
    }
    fn launch(&mut self, args: Vec<String>, purpose: &str) {
        if self.rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let purpose = purpose.to_string();
        self.rx = Some(rx);
        self.request_page = self.page;
        self.message = "正在执行…".into();
        std::thread::spawn(move || {
            let _ = tx.send(execute(&args, &purpose));
        });
    }
    fn base(&self, words: &[&str]) -> Vec<String> {
        let mut args = vec!["--vm".into(), self.vm.clone()];
        args.extend(words.iter().map(|x| x.to_string()));
        args
    }
    fn preview(&mut self, mut args: Vec<String>) {
        args.push("--dry-run".into());
        self.pending = None;
        self.launch(args, "preview");
    }
    fn poll(&mut self) {
        let reply = self.rx.as_ref().and_then(|r| r.try_recv().ok());
        if let Some(r) = reply {
            let origin = self.request_page;
            let previous = (self.output.clone(), self.message.clone(), self.good);
            self.handle_reply(r);
            if origin != self.page {
                self.output = previous.0;
                self.message = previous.1;
                self.good = previous.2;
            }
        }
    }
    fn handle_reply(&mut self, r: Reply) {
        self.rx = None;
        // Replies belong to the VM named in their immutable request.
        if let Some(i) = r.args.iter().position(|a| a == "--vm") {
            if r.args.get(i + 1) != Some(&self.vm) {
                return;
            }
        }
        if r.purpose == "local-services" {
            if r.ok {
                match serde_json::from_str::<serde_json::Value>(&r.text) {
                    Ok(v) if v["services"].is_array()=>{self.local_services.data=v;self.local_services.loaded=Some(std::time::Instant::now());self.local_services.error.clear();},
                    _=>self.local_services.error="无法解析 Mac 转发服务列表".into(),
                }
            }else{self.local_services.error=r.text;}
            return;
        }
        if matches!(r.purpose.as_str(),"local-service-change"|"local-check") {
            let target_failed=r.purpose=="local-check" && serde_json::from_str::<serde_json::Value>(&r.text).map(|v|v["target_tcp_ok"]==false).unwrap_or(false);
            self.good=r.ok&&!target_failed;
            self.message=if target_failed{"检查发现 Mac 到实际目标不可达，请查看操作结果"}else if r.ok {"Mac 转发操作完成"}else{"Mac 转发操作失败，请查看操作结果"}.into();
            self.local_services.result=serde_json::from_str::<serde_json::Value>(&r.text).ok().and_then(|v|serde_json::to_string_pretty(&v).ok()).unwrap_or(r.text);
            self.local_services.refresh=true;self.topology_services.refresh=true;
            return;
        }
        if r.purpose == "service-change" {
            self.good = r.ok;
            self.message = if r.ok {
                "规则操作完成"
            } else {
                "规则操作失败，请查看逐条结果"
            }
            .into();
            self.services.result = serde_json::from_str::<serde_json::Value>(&r.text)
                .ok()
                .and_then(|v| serde_json::to_string_pretty(&v).ok())
                .unwrap_or(r.text);
            self.services.refresh = true;
            self.topology_services.refresh = true;
            return;
        }
        if r.purpose.starts_with("routes-") {
            self.routes_reply(r);
            return;
        }
        if r.purpose.starts_with("host-") {
            self.host_reply(r);
            return;
        }
        self.good = r.ok;
        self.message = if r.ok {
            "完成"
        } else {
            "操作失败，请查看详情"
        }
        .into();
        if r.purpose == "vms" && r.ok {
            match serde_json::from_str::<serde_json::Value>(&r.text) {
                Ok(serde_json::Value::Array(rows)) => {
                    self.vms = rows.iter().map(guest::Guest::parse).collect();
                    self.output = "已读取虚拟机列表。可以查看每台虚拟机的系统与运行状态。".into();
                }
                _ => {
                    self.output = r.text;
                    self.good = false;
                    self.message = "无法解析虚拟机列表，可手动输入名称".into();
                }
            }
        } else {
            if r.purpose == "addresses" {
                self.address_rows.clear();
                if r.ok {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&r.text) {
                        self.address_rows = hostnet::array(&value).to_vec();
                    }
                }
            }
            if r.purpose == "containers" && r.ok {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&r.text) {
                    self.containers = value
                        .get("containers")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                }
            }
            if r.purpose == "services" {
                if r.ok {
                    match serde_json::from_str::<serde_json::Value>(&r.text) {
                        Ok(v) if v["services"].is_array() => {
                            self.services.data = v;
                            self.services.loaded = Some(std::time::Instant::now());
                            self.services.error.clear();
                        }
                        _ => {
                            self.services.error = "无法解析服务列表".into();
                            self.good = false;
                        }
                    }
                } else {
                    self.services.error = r.text.clone();
                }
            }
            if r.purpose == "apply" && r.ok {
                self.services.refresh = true;
                self.topology_services.refresh = true;
            }
            self.output = r.text;
            if r.purpose == "preview" && r.ok {
                self.pending = Some(r.args.into_iter().filter(|s| s != "--dry-run").collect());
            }
            self.log.push(format!(
                "{} · {}",
                r.purpose,
                if r.ok { "完成" } else { "失败" }
            ));
        }
    }
    fn form_row(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
        ui.label(RichText::new(label).strong());
        ui.add(
            egui::TextEdit::singleline(value)
                .desired_width(410.)
                .hint_text(hint),
        );
    }
    fn endpoint(&self, p: &str) -> String {
        format!("{}:{}", self.listen_ip.trim(), p.trim())
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        self.services_tick(ctx);
        self.topology_services_tick(ctx);
        if self.page == Page::Topology {
            ctx.request_repaint_after(Duration::from_secs(1));
            if self.topology.auto
                && self.rx.is_none()
                && self
                    .topology
                    .attempt
                    .map(|t| t.elapsed() >= Duration::from_secs(10))
                    .unwrap_or(true)
            {
                self.topology.attempt = Some(std::time::Instant::now());
                self.launch(vec![], "host-read");
            }
        }
        if self.rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
        self.desktop_shell(ctx);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::WHITE)).show(ctx,|ui|{
            if matches!(self.page,Page::Forward|Page::Socks){self.sync_selection();self.forward_workspace_ui(ui,self.page);return;}
            egui::Frame::new().inner_margin(egui::Margin::same(22)).show(ui,|ui|{
            if self.page==Page::Topology {egui::ScrollArea::vertical().show(ui,|ui|self.topology_ui(ui));return;}
            egui::ScrollArea::vertical().show(ui,|ui|{
                ui.add_space(12.);ui.heading(match self.page{Page::Topology=>"网络拓扑",Page::Overview=>"网络概览",Page::Host=>"本机与虚拟机网络",Page::Forward=>"端口转发",Page::Socks=>"SOCKS5 代理",Page::Vpn=>"VPN 连接",Page::Containers=>"容器状态",Page::Services=>"转发与代理监控",Page::Routes=>"默认出口与路由"});
                ui.label("让 Mac 与虚拟机之间的连接更清楚、更可控。");ui.add_space(10.);
                ui.add_enabled_ui(self.rx.is_none(),|ui|{
                    ui.horizontal(|ui|{
                        ui.label(if self.page==Page::Forward {if self.local_forward{"关联虚拟机（用于填写目标）"}else{"运行转发的虚拟机"}}else{"虚拟机"});
                        let selected_text=self.vms.iter().find(|g|g.name==self.vm).map(vm_choice_text).unwrap_or_else(||RichText::new(if self.vm.is_empty(){"请选择虚拟机"}else{&self.vm}));
                        egui::ComboBox::from_id_salt("vm-list").selected_text(selected_text).width(340.).show_ui(ui,|ui|{
                            let running=self.vms.iter().filter(|g|g.state=="running").count();
                            ui.small(format!("运行中 {running} / 共 {} 台 · 点击刷新列表更新状态",self.vms.len()));
                            ui.separator();
                            if self.vms.is_empty(){ui.label("暂无虚拟机，请刷新列表。");}
                            let mut guests=self.vms.iter().collect::<Vec<_>>();
                            guests.sort_by_key(|g|g.state!="running");
                            for g in guests{
                                if ui.selectable_value(&mut self.vm,g.name.clone(),vm_choice_text(g)).on_hover_text(format!("{}\n运行状态来自 Parallels；网络连通及 Tools 可用性需分别检查。",g.summary())).changed(){self.pending=None;self.containers.clear();}
                            }
                        });
                        if ui.button("刷新列表").clicked(){self.launch(vec![],"vms");}
                    });
                    if ui.add(egui::TextEdit::singleline(&mut self.vm).hint_text("也可输入精确的虚拟机名称").desired_width(440.)).changed(){self.pending=None;self.containers.clear();}
                });
                ui.separator();
                if self.page==Page::Overview {
                    ui.strong("虚拟机运行状态");
                    egui::Grid::new("vm-table").striped(true).num_columns(4).show(ui,|ui|{
                        ui.strong("名称");ui.strong("系统");ui.strong("状态");ui.strong("Tools");ui.end_row();
                        for g in &self.vms {
                            if ui.selectable_label(self.vm==g.name,&g.name).clicked(){self.vm=g.name.clone();self.pending=None;self.containers.clear();}
                            ui.label(&g.os);
                            ui.colored_label(vm_state_color(&g.state),state_label(&g.state));
                            ui.label(&g.tools);ui.end_row();
                        }
                    });
                    ui.separator();
                }
                self.sync_selection();
                let selected=self.vms.iter().find(|g|g.name==self.vm);
                let block=selected.and_then(|g|g.check().err());
                if let Some(g)=selected {
                    ui.label(format!("系统：{}    状态：{}    Tools：{}",g.os,state_label(&g.state),g.tools)).on_hover_text(g.summary());
                }
                if self.page!=Page::Host {if let Some(reason)=&block {
                    ui.colored_label(Color32::from_rgb(170,70,30),reason);

                }
                }
                if let Some(g)=selected {if g.tools=="possibly_installed" {ui.small("Tools 可能安装过，但尚未确认服务可用；执行失败时请检查来宾 Tools。");}}
                if self.page==Page::Host {self.network_ui(ui);}
                if matches!(self.page,Page::Forward|Page::Socks) {self.service_manager(ui,self.page);}
                if self.page==Page::Services {self.services_ui(ui);}
                ui.add_enabled_ui(self.rx.is_none()&&!self.vm.is_empty()&&block.is_none(),|ui|{
                    match self.page{
                        Page::Host|Page::Topology=>{},
                        Page::Overview=>{
                            ui.label("查看网卡、IPv4 地址、路由、VPN 状态及现有转发。");
                            if ui.button(RichText::new("读取状态").color(BLUE)).clicked(){self.launch(self.base(&["status"]),"read");}
                            ui.small("虚拟机必须已启动，安装 Parallels Tools，并具有执行管理命令的权限。");
                        }
                        Page::Forward|Page::Socks|Page::Services=>{},
                        Page::Routes=>self.routes_ui(ui),
                        Page::Containers=>{
                            ui.label("查看所选虚拟机中的 Docker / Podman 容器，包含运行中和已停止的容器。");
                            let running=self.containers.iter().filter(|r|r.get("state").and_then(|s|s.as_str())==Some("running")).count();
                            ui.strong(format!("运行中 {running} / 全部 {}",self.containers.len()));
                            ui.small("只读列表；未安装引擎或守护进程不可用会在执行详情中说明。不会启动容器或 Docker。");
                            if ui.button("刷新容器状态").clicked(){self.containers.clear();self.launch(self.base(&["containers","list"]),"containers");}
                            egui::ScrollArea::horizontal().show(ui,|ui|{egui::Grid::new("container-table").striped(true).num_columns(5).show(ui,|ui|{
                                for h in ["引擎","容器名称","镜像","运行状态","端口"]{ui.strong(h);}ui.end_row();
                                for row in &self.containers {
                                    for k in ["engine","name","image","status","ports"] {
                                        let value=row.get(k).map(|v|v.as_str().map(str::to_owned).unwrap_or_else(||v.to_string())).unwrap_or_default();
                                        if k=="status" {let running=row.get("state").and_then(|v|v.as_str()).map(|x|x=="running").unwrap_or(value.starts_with("Up"));ui.colored_label(if running{Color32::from_rgb(24,120,84)}else{Color32::GRAY},value);} else {ui.label(value);}
                                    }ui.end_row();
                                }
                            });});
                        }
                        Page::Vpn=>{
                            ui.label("连接或断开已有 VPN。Windows 使用 rasdial；Linux 使用 NetworkManager；macOS 使用 scutil。使用已有凭据。");
                            Self::form_row(ui,"VPN 连接名称",&mut self.vpn,"例如 比赛 VPN");
                            ui.horizontal(|ui|{
                                if ui.button("读取 VPN 列表").clicked(){self.launch(self.base(&["vpn","list"]),"read");}
                                for (label,op) in [("预览连接","connect"),("预览断开","disconnect")]{
                                    if ui.add_enabled(!self.vpn.trim().is_empty(),egui::Button::new(label)).clicked(){let mut a=self.base(&["vpn",op]);a.extend(["--name".into(),self.vpn.trim().into()]);self.preview(a);}
                                }
                            });
                            ui.small("VPN 可能改变默认路由。本工具不会自动增删路由，也不会自动创建 VPN。");
                        }
                    }
                });
                ui.add_space(12.);ui.separator();
                self.network_preview_ui(ui);
                self.service_preview_ui(ui);
                if matches!(self.page,Page::Services|Page::Forward|Page::Socks|Page::Routes) {
                    let detail=if matches!(self.page,Page::Services|Page::Forward|Page::Socks){serde_json::to_string_pretty(&serde_json::json!({"mac_local":self.local_services.data,"selected_vm":self.services.data})).unwrap_or_default()}else{self.output.clone()};
                    egui::CollapsingHeader::new("原始响应 / 排查详情").show(ui,|ui|{
                        if ui.button("复制原始响应").clicked(){ui.ctx().copy_text(detail.clone());}
                        egui::ScrollArea::both().max_height(220.).show(ui,|ui|{ui.monospace(&detail);});
                    });
                    return;
                }
                ui.horizontal(|ui|{ui.strong("执行详情");if ui.button("复制").clicked(){ui.ctx().copy_text(self.output.clone());}});
                egui::ScrollArea::both().id_salt("output-scroll").max_height(340.).show(ui,|ui|{ui.add(egui::TextEdit::multiline(&mut self.output).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY).desired_rows(13).interactive(false));});
                egui::CollapsingHeader::new("本次操作记录").show(ui,|ui|{for entry in self.log.iter().rev().take(30){ui.label(entry);}});
            });
            });
        });
        self.topology_tool_ui(ctx);
    }
}
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1320., 880.])
            .with_min_inner_size([1100., 720.]),
        ..Default::default()
    };
    eframe::run_native(
        "VNet Studio · 虚拟机网络管理",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
