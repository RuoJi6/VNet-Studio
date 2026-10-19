use crate::hostnet::{self, array, field};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use serde_json::Value;
use std::{collections::BTreeMap, time::Instant};
const BLUE: Color32 = Color32::from_rgb(55, 105, 220);
const TEAL: Color32 = Color32::from_rgb(25, 139, 130);
const PURPLE: Color32 = Color32::from_rgb(128, 91, 194);
const HIGHLIGHT: Color32 = Color32::from_rgb(222, 104, 20);
const GRAY: Color32 = Color32::from_rgb(133, 142, 157);
#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub lines: Vec<String>,
    pub detail: String,
    pub vm: Option<String>,
    pub service: Option<Value>,
    pub column: usize,
    pub color: Color32,
}
#[derive(Clone, Debug)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub label: String,
    pub connected: bool,
    pub color: Color32,
}
#[derive(Default)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}
fn node(
    id: &str,
    title: &str,
    kind: &str,
    status: &str,
    column: usize,
    color: Color32,
    lines: Vec<String>,
    detail: Value,
) -> Node {
    Node {
        id: id.into(),
        title: title.into(),
        kind: kind.into(),
        status: status.into(),
        column,
        color,
        lines,
        detail: serde_json::to_string_pretty(&detail).unwrap_or_default(),
        vm: None,
        service: None,
    }
}
fn edge(from: &str, to: &str, label: &str, connected: bool, color: Color32) -> Edge {
    Edge {
        from: from.into(),
        to: to.into(),
        label: label.into(),
        connected,
        color,
    }
}
fn vm_state(s: &str) -> &str {
    match s {
        "running" => "运行中",
        "stopped" => "已停止",
        "suspended" => "已挂起",
        "paused" => "已暂停",
        _ => "状态未知",
    }
}
fn usb_owner<'a>(s: &'a Value, u: &Value) -> Option<&'a Value> {
    let owner = field(u, "Connected-To-Vm").trim_matches(['{', '}']);
    array(&s["vms"]).iter().find(|v| {
        !owner.is_empty()
            && (field(v, "ID").trim_matches(['{', '}']) == owner || field(v, "Name") == owner)
    })
}
pub fn build(s: &Value, query: &str, running_only: bool, show_inactive: bool) -> Graph {
    let mut nodes = BTreeMap::new();
    let mut edges = vec![];
    nodes.insert(
        "host".into(),
        node(
            "host",
            "本机 Mac",
            "宿主机",
            "网络快照",
            0,
            BLUE,
            vec!["Parallels Desktop".into(), "连接关系 / 接口状态".into()],
            serde_json::json!({"说明":"连线表示设备归属或网络配置，不表示互联网和服务已经连通"}),
        ),
    );
    let query = query.to_lowercase();
    for v in array(&s["vms"]) {
        let name = field(v, "Name");
        let id = format!("vm:{}", field(v, "ID"));
        let state = field(v, "State");
        let ips = array(&v["Network"]["ipAddresses"])
            .iter()
            .filter(|a| field(a, "type") == "ipv4")
            .map(|a| field(a, "ip"))
            .collect::<Vec<_>>()
            .join(", ");
        if (running_only && state != "running")
            || !format!("{name} {} {ips}", field(v, "OS"))
                .to_lowercase()
                .contains(&query)
        {
            continue;
        }
        let mut n = node(
            &id,
            name,
            "虚拟机",
            vm_state(state),
            2,
            if state == "running" { TEAL } else { GRAY },
            vec![
                format!("{} · {} 张网卡", field(v, "OS"), hostnet::adapters(v).len()),
                if ips.is_empty() {
                    "PD 未报告 IPv4".into()
                } else {
                    format!("IP：{ips}")
                },
                format!("Tools：{}", field(&v["GuestTools"], "state")),
            ],
            v.clone(),
        );
        n.vm = Some(name.into());
        nodes.insert(id.clone(), n);
        for (dev, a) in hostnet::adapters(v) {
            let typ = field(&a, "type");
            let (nid, title, color) = match typ {
                "shared" => ("network:shared".into(), "共享网络 · NAT".into(), BLUE),
                "host-only" => ("network:host-only".into(), "仅主机网络".into(), PURPLE),
                "bridged" => {
                    let iface = hostnet::bridge_interface(s, &a);
                    if !iface.is_empty()
                        && array(&s["interfaces"])
                            .iter()
                            .any(|i| field(i, "device") == iface)
                    {
                        (format!("iface:{iface}"), format!("桥接 · {iface}"), TEAL)
                    } else {
                        (
                            format!("unknown:{iface}"),
                            format!(
                                "桥接 · {}",
                                if iface.is_empty() {
                                    "自动 / 未报告"
                                } else {
                                    &iface
                                }
                            ),
                            GRAY,
                        )
                    }
                }
                _ => (format!("unknown:{typ}"), format!("网络类型：{typ}"), GRAY),
            };
            if !nodes.contains_key(&nid) {
                let desc = match typ {
                    "shared" => "通过 Mac 默认网络出口",
                    "host-only" => "Mac 与虚拟机间通信",
                    _ => "接口映射待确认",
                };
                nodes.insert(
                    nid.clone(),
                    node(
                        &nid,
                        &title,
                        "虚拟网络",
                        if nid.starts_with("unknown:") {
                            "未确认"
                        } else {
                            "已配置"
                        },
                        1,
                        color,
                        vec![desc.into()],
                        a.clone(),
                    ),
                );
                edges.push(edge(
                    "host",
                    &nid,
                    if typ == "shared" {
                        "NAT 配置"
                    } else if typ == "host-only" {
                        "宿主连接依配置"
                    } else {
                        "桥接配置"
                    },
                    !nid.starts_with("unknown:"),
                    color,
                ));
            }
            let connected =
                state == "running" && a["enabled"] == true && field(&a, "state") != "disconnected";
            let label = format!(
                "{dev} · {}",
                if a["enabled"] == false {
                    "已禁用"
                } else if field(&a, "state") == "disconnected" {
                    "已断开"
                } else if state != "running" {
                    "VM 未运行"
                } else if a["enabled"] != true {
                    "状态未知"
                } else {
                    "已连接（配置）"
                }
            );
            edges.push(edge(&nid, &id, &label, connected, color));
        }
    }
    for i in array(&s["interfaces"]) {
        let dev = field(i, "device");
        let id = format!("iface:{dev}");
        let linked = nodes.contains_key(&id);
        if !linked && (!query.is_empty() || running_only) {
            continue;
        }
        if !linked && !show_inactive && field(i, "state") != "active" {
            continue;
        }
        if dev == "lo0" {
            continue;
        }
        let ips = array(&i["ips"])
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let active = field(i, "state") == "active";
        nodes.insert(
            id.clone(),
            node(
                &id,
                field(i, "name"),
                "Mac 接口",
                if active {
                    "链路已连接"
                } else if field(i, "state") == "inactive" {
                    "链路未连接"
                } else {
                    "链路未知"
                },
                1,
                if active { TEAL } else { GRAY },
                vec![
                    dev.into(),
                    if ips.is_empty() {
                        "未报告 IPv4".into()
                    } else {
                        ips
                    },
                ],
                i.clone(),
            ),
        );
        edges.retain(|e| !(e.from == "host" && e.to == id));
        edges.push(edge("host", &id, "本机接口", active, GRAY));
    }
    for u in array(&s["usb"]) {
        let owner = usb_owner(s, u);
        let to = owner.map(|v| format!("vm:{}", field(v, "ID")));
        let name = field(u, "Name");
        let is_net = ["lan", "ethernet", "network", "网卡"]
            .iter()
            .any(|k| name.to_lowercase().contains(k));
        if !is_net && to.is_none() {
            continue;
        }
        if (!query.is_empty() || running_only)
            && !to
                .as_ref()
                .map(|id| nodes.contains_key(id))
                .unwrap_or(false)
        {
            continue;
        }
        let connected = !matches!(field(u, "Connected-To-Vm"), "" | "NO");
        let present = field(u, "Present-On-Host") == "YES";
        if !show_inactive && !present && !connected {
            continue;
        }
        let id = format!("usb:{}", field(u, "System name"));
        nodes.insert(
            id.clone(),
            node(
                &id,
                name,
                "USB 设备",
                if connected {
                    "连接至虚拟机"
                } else if present {
                    "已插入 Mac"
                } else {
                    "未插入"
                },
                1,
                if connected { PURPLE } else { GRAY },
                vec![
                    if connected && owner.is_none() {
                        "当前所属 VM 未报告".into()
                    } else {
                        "独占直通设备".into()
                    },
                    format!("插入时行为：{}", field(u, "Autoconnect-Action")),
                ],
                u.clone(),
            ),
        );
        if let Some(to) = to.filter(|id| nodes.contains_key(id)) {
            edges.push(edge(&id, &to, "USB 直通", true, PURPLE));
        } else if !connected {
            edges.push(edge("host", &id, "USB 设备", present, GRAY));
        }
    }
    Graph {
        nodes: nodes.into_values().collect(),
        edges,
    }
}
const W: f32 = 300.;
const H: f32 = 132.;
impl Graph {
    pub fn positions(&self) -> BTreeMap<String, Pos2> {
        let mut result = BTreeMap::new();
        let columns = self.nodes.iter().map(|n| n.column).max().unwrap_or(0) + 1;
        let mut counts = vec![0; columns];
        for n in &self.nodes {
            counts[n.column] += 1;
        }
        let height = (*counts.iter().max().unwrap_or(&1) as f32) * 170.;
        let mut index = vec![0; columns];
        for n in &self.nodes {
            let y = (height - counts[n.column] as f32 * 170.) / 2. + index[n.column] as f32 * 170.;
            result.insert(n.id.clone(), egui::pos2(n.column as f32 * 390., y));
            index[n.column] += 1;
        }
        result
    }
}
fn curve_point(points: &[Pos2;4], t:f32) -> Pos2 {
    let q=1.-t;
    (points[0].to_vec2()*q.powi(3)+points[1].to_vec2()*3.*q*q*t+points[2].to_vec2()*3.*q*t*t+points[3].to_vec2()*t.powi(3)).to_pos2()
}
fn curve_distance(points:&[Pos2;4], pos:Pos2) -> f32 {
    let mut last=points[0]; let mut distance=f32::INFINITY;
    for j in 1..=64 {
        let next=curve_point(points,j as f32/64.);
        let segment=next-last;
        let t=if segment.length_sq()>0. {((pos-last).dot(segment)/segment.length_sq()).clamp(0.,1.)}else{0.};
        distance=distance.min(pos.distance(last+segment*t)); last=next;
    }
    distance
}
fn edge_key(e:&Edge)->(String,String,String) {(e.from.clone(),e.to.clone(),e.label.clone())}
pub struct View {
    pub zoom: f32,
    pub pan: Vec2,
    pub selected: String,
    pub selected_edge: Option<(String,String,String)>,
    pub action: Option<(String, String)>,
    pub query: String,
    pub running_only: bool,
    pub show_inactive: bool,
    pub auto: bool,
    pub access_view: bool,
    pub loaded: Option<Instant>,
    pub attempt: Option<Instant>,
    pub error: String,
    offsets: BTreeMap<String, Vec2>,
    fit: bool,
    columns: usize,
    drag: Option<(String, Pos2)>,
}
impl Default for View {
    fn default() -> Self {
        Self {
            zoom: 0.75,
            pan: Vec2::ZERO,
            selected: String::new(),
            selected_edge: None,
            action: None,
            query: String::new(),
            running_only: false,
            show_inactive: false,
            auto: false,
            access_view: true,
            loaded: None,
            attempt: None,
            error: String::new(),
            offsets: BTreeMap::new(),
            fit: true,
            columns: 0,
            drag: None,
        }
    }
}
impl View {
    pub fn reset(&mut self) {
        self.selected_edge=None;
        self.offsets.clear();
        self.drag = None;
        self.fit = true;
    }
    fn move_pointer(&mut self, pos: Pos2) {
        if let Some((id, last)) = &mut self.drag {
            let delta = pos - *last;
            if id.is_empty() {
                self.pan += delta;
            } else {
                *self.offsets.entry(id.clone()).or_default() += delta / self.zoom;
            }
            *last = pos;
        }
    }
    pub fn canvas(&mut self, ui: &mut egui::Ui, g: &Graph) {
        let height = (ui.ctx().screen_rect().height() - 330.).clamp(320., 800.);
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::click_and_drag(),
        );
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 10., Color32::from_rgb(244, 247, 251));
        let positions = g.positions();
        let columns = g.nodes.iter().map(|n| n.column).max().unwrap_or(0);
        if self.columns != columns {
            self.columns = columns;
            self.fit = true;
        }
        if self.fit {
            let maxy = positions.values().map(|v| v.y + H).fold(H, f32::max);
            let width = positions.values().map(|v| v.x + W).fold(W, f32::max);
            self.zoom = ((rect.width() - 40.) / width)
                .min((rect.height() - 50.) / maxy)
                .clamp(0.35, 1.2);
            self.pan = egui::vec2(((rect.width() - width * self.zoom) / 2.).max(20.), 25.);
            self.fit = false;
        }

        if response.hovered() {
            let factor = ui.input(|i| i.zoom_delta());
            if (factor - 1.).abs() > 0.001 {
                self.zoom = (self.zoom * factor).clamp(0.35, 1.6);
            }
        }
        let step = 22. * self.zoom;
        let mut x = rect.left() + self.pan.x.rem_euclid(step);
        while x < rect.right() {
            let mut y = rect.top() + self.pan.y.rem_euclid(step);
            while y < rect.bottom() {
                p.circle_filled(egui::pos2(x, y), 0.7, Color32::from_rgb(207, 217, 230));
                y += step;
            }
            x += step;
        }
        let boxes: BTreeMap<String, Rect> = positions
            .iter()
            .map(|(id, pos)| {
                let offset = self.offsets.get(id).copied().unwrap_or_default();
                (
                    id.clone(),
                    Rect::from_min_size(
                        rect.min + self.pan + (pos.to_vec2() + offset) * self.zoom,
                        egui::vec2(W, H) * self.zoom,
                    ),
                )
            })
            .collect();
        // Use ordered pointer events so fast native drags are not lost between frames.
        for event in ui.input(|i| i.events.clone()) {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } if rect.contains(pos) && ui.rect_contains_pointer(rect) => {
                    let id = boxes
                        .iter()
                        .rev()
                        .find(|(_, r)| r.contains(pos))
                        .map(|(id, _)| id.clone())
                        .unwrap_or_default();
                    self.drag = Some((id, pos));
                }
                egui::Event::PointerMoved(pos) => self.move_pointer(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => {
                    self.move_pointer(pos);
                    self.drag = None;
                }
                egui::Event::PointerGone => {
                    self.drag = None;
                }
                _ => {}
            }
        }
        if self.drag.is_some() {
            ui.ctx().request_repaint();
        }
        if self.selected_edge.as_ref().is_some_and(|key|!g.edges.iter().any(|e|edge_key(e)==*key)) {self.selected_edge=None;}
        if ui.input(|i|i.key_pressed(egui::Key::Escape)){self.selected_edge=None;}
        let font=egui::FontId::proportional((11.*self.zoom).max(9.));
        let mut routes=Vec::new();
        for (ei,e) in g.edges.iter().enumerate() {
            let (Some(a),Some(b))=(boxes.get(&e.from),boxes.get(&e.to))else{continue;};
            let start=a.right_center();
            let count=g.edges.iter().filter(|other|other.to==e.to).count();
            let rank=g.edges[..ei].iter().filter(|other|other.to==e.to).count();
            let end=egui::pos2(b.left(),b.top()+b.height()*(rank+1) as f32/(count+1) as f32);
            let middle=(start.x+end.x)/2.;
            let points=[start,egui::pos2(middle,start.y),egui::pos2(middle,end.y),end];
            let text=if self.zoom<0.65&&e.label.contains("net"){e.label.split('·').next().unwrap_or(&e.label).to_string()}else{e.label.clone()};
            let label=p.layout_no_wrap(text.clone(),font.clone(),e.color);
            let at=egui::pos2(end.x-label.size().x-8.,end.y-18.);
            routes.push((ei,points,text,Rect::from_min_size(at,label.size()).expand(2.)));
        }
        // Pick the closest actual curve (including dashed gaps), not its bounding box.
        // Cards take precedence over routes; tolerance is constant in screen points.
        let hovered=ui.input(|i|i.pointer.hover_pos()).filter(|pos|rect.contains(*pos)&&ui.rect_contains_pointer(rect)&&!boxes.values().any(|r|r.contains(*pos))).and_then(|pos|{
            routes.iter().filter_map(|(ei,points,_,label)|{
                let distance=if label.contains(pos){0.}else{curve_distance(points,pos)};
                (distance<=7.).then_some((*ei,distance))
            }).min_by(|a,b|a.1.total_cmp(&b.1)).map(|(ei,_)|ei)
        });
        if response.clicked() {
            if let Some(ei)=hovered {self.selected_edge=Some(edge_key(&g.edges[ei]));}
            else {self.selected.clear();self.selected_edge=None;}
        }
        if let Some(ei)=hovered {
            let e=&g.edges[ei];let name=|id:&str|g.nodes.iter().find(|n|n.id==id).map(|n|n.title.as_str()).unwrap_or("未知");
            response.clone().on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("{} → {}\n{}\n点击高亮此路线；点击空白取消",name(&e.from),name(&e.to),e.label));
        }
        let selected=self.selected_edge.as_ref().and_then(|key|g.edges.iter().position(|e|edge_key(e)==*key));
        // Draw the chosen route last so crossing lines and labels cannot cover it.
        routes.sort_by_key(|(ei,_,_,_)|if Some(*ei)==selected{2}else if Some(*ei)==hovered{1}else{0});
        for (ei,points,text,label_rect) in routes {
            let e=&g.edges[ei]; let active=Some(ei)==selected;let hover=Some(ei)==hovered;
            let base=if e.connected||e.label.starts_with("规则"){e.color}else{GRAY};
            let color=if active{HIGHLIGHT}else if hover{Color32::from_rgb(199,125,65)}else if selected.is_some(){Color32::from_rgb(211,218,227)}else{base};
            let width=if active{3.5}else if hover{2.5}else{1.5};
            if e.connected {
                p.add(egui::epaint::CubicBezierShape::from_points_stroke(points,false,Color32::TRANSPARENT,Stroke::new(width,color)));
            } else {
                let mut last=points[0];
                for j in 1..=64 {let next=curve_point(&points,j as f32/64.);if j%4<2{p.line_segment([last,next],Stroke::new(width,color));}last=next;}
            }
            p.circle_filled(points[3],if active{4.5}else{3.},color);
            let label=p.layout_no_wrap(text,font.clone(),color);
            p.rect_filled(label_rect,3.,if active{Color32::from_rgb(255,239,218)}else{Color32::from_rgb(244,247,251)});
            p.galley(label_rect.min+egui::vec2(2.,2.),label,color);
        }
        for n in &g.nodes {
            let r = boxes[&n.id];
            if !rect.intersects(r) {
                continue;
            }
            let hit = ui.interact(
                r.intersect(rect),
                ui.id().with(&n.id),
                egui::Sense::click_and_drag(),
            );
            if hit.clicked() {
                self.selected = n.id.clone();self.selected_edge=None;
            }

            if let Some(vm) = &n.vm {
                if hit.double_clicked() {
                    self.selected = n.id.clone();
                    self.action = Some((vm.clone(), "services".into()));
                }
                hit.context_menu(|ui| {
                    for (key, label) in [
                        ("network", "网络设置"),
                        ("routes", "默认出口 / 路由"),
                        ("forward", "添加端口转发"),
                        ("socks", "配置 SOCKS5"),
                        ("services", "运行与通信日志"),
                    ] {
                        if ui.button(label).clicked() {
                            self.selected = n.id.clone();
                            self.action = Some((vm.clone(), key.into()));
                            ui.close_menu();
                        }
                    }
                });
            }
            let endpoint=self.selected_edge.as_ref().is_some_and(|(from,to,_)|from==&n.id||to==&n.id);
            let selected = self.selected_edge.is_none() && self.selected == n.id;
            p.rect_filled(
                r.translate(egui::vec2(0., 3.)),
                8.,
                Color32::from_black_alpha(9),
            );
            p.rect_filled(r, 8., if endpoint{Color32::from_rgb(255,248,238)}else{Color32::WHITE});
            p.rect_stroke(
                r,
                8.,
                Stroke::new(
                    if endpoint { 2.5 } else if selected { 2. } else { 1. },
                    if endpoint { HIGHLIGHT } else if selected {
                        BLUE
                    } else {
                        Color32::from_rgb(218, 226, 237)
                    },
                ),
                egui::StrokeKind::Inside,
            );
            p.rect_filled(
                Rect::from_min_size(r.min, egui::vec2(4. * self.zoom, r.height())),
                3.,
                n.color,
            );
            let p = p.with_clip_rect(r.intersect(rect));
            let x = r.left() + 13. * self.zoom;
            let mut y = r.top() + 10. * self.zoom;
            let font = (13. * self.zoom).max(10.);
            p.text(
                egui::pos2(x, y),
                egui::Align2::LEFT_TOP,
                format!(
                    "{} · {}",
                    n.kind,
                    if self.zoom < 0.65 {
                        n.status.split(['（', ' ']).next().unwrap_or(&n.status)
                    } else {
                        &n.status
                    }
                ),
                egui::FontId::proportional(font - 1.),
                n.color,
            );
            y += 22. * self.zoom;
            let mut job = egui::text::LayoutJob::simple(
                n.title.clone(),
                egui::FontId::proportional(font + 1.),
                Color32::from_rgb(30, 44, 65),
                r.width() - 25. * self.zoom,
            );
            job.wrap.max_rows = 2;
            job.wrap.break_anywhere = true;
            let title = p.layout_job(job);
            y += title.size().y + 7. * self.zoom;
            p.galley(
                egui::pos2(x, r.top() + 32. * self.zoom),
                title,
                Color32::BLACK,
            );
            for line in n.lines.iter().take(if self.zoom < 0.65 { 0 } else { 3 }) {
                let mut job = egui::text::LayoutJob::simple(
                    line.clone(),
                    egui::FontId::proportional(font - 1.),
                    Color32::from_rgb(78, 91, 109),
                    r.width() - 25. * self.zoom,
                );
                job.wrap.max_rows = 1;
                job.wrap.break_anywhere = true;
                let label = p.layout_job(job);
                p.galley(egui::pos2(x, y), label, Color32::BLACK);
                y += 18. * self.zoom;
            }
            if self.zoom < 0.65 && self.zoom >= 0.35 {
                let summary = if n.vm.is_some() || n.kind == "Mac 接口" {
                    n.lines.get(1)
                } else {
                    n.lines.first()
                };
                if let Some(summary) = summary {
                    let mut job = egui::text::LayoutJob::simple(
                        summary.clone(),
                        egui::FontId::proportional(9.),
                        Color32::from_rgb(78, 91, 109),
                        r.width() - 20.,
                    );
                    job.wrap.max_rows = 1;
                    job.wrap.break_anywhere = true;
                    p.galley(
                        egui::pos2(x, r.bottom() - 15.),
                        p.layout_job(job),
                        Color32::BLACK,
                    );
                }
            }
            hit.on_hover_text(format!(
                "{}\n{}\n点击查看详情，拖动调整位置",
                n.title,
                n.lines.join("\n")
            ));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> Value {
        json!({"vms":[{"ID":"a","Name":"Linux","State":"running","OS":"ubuntu","Hardware":{"net0":{"type":"shared","enabled":true},"net1":{"type":"bridged","iface":"USB LAN","enabled":true,"state":"disconnected"}}},{"ID":"b","Name":"Windows","State":"suspended","Hardware":{"net0":{"type":"host","enabled":true}}}],"interfaces":[{"name":"USB LAN","device":"en7","state":"active","ips":["10.0.0.2"]}],"usb":[]})
    }
    #[test]
    fn actual_modes_and_link_states() {
        let g = build(&fixture(), "", false, false);
        assert!(g.nodes.iter().any(|n| n.id == "network:host-only"));
        assert!(g
            .edges
            .iter()
            .any(|e| e.from == "iface:en7" && e.to == "vm:a" && !e.connected));
        assert!(g.edges.iter().any(|e| e.to == "vm:b" && !e.connected));
        assert!(g
            .edges
            .iter()
            .any(|e| e.from == "network:shared" && e.to == "vm:a" && e.connected));
        for e in &g.edges {
            assert!(g.nodes.iter().any(|n| n.id == e.from));
            assert!(g.nodes.iter().any(|n| n.id == e.to));
        }
    }
    #[test]
    fn no_usb_owner_inferred_from_autoconnect() {
        let mut s = fixture();
        s["usb"] = json!([{"Name":"USB LAN","System name":"u","Connected-To-Vm":"NO","Present-On-Host":"NO","Autoconnect-Vm-Uuid":"a"}]);
        let g = build(&s, "", false, true);
        assert!(!g.edges.iter().any(|e| e.from == "usb:u" && e.to == "vm:a"));
    }
    #[test]
    fn filtering_and_layout() {
        let g = build(&fixture(), "linux", true, false);
        assert!(!g.nodes.iter().any(|n| n.id == "vm:b"));
        let p = g.positions();
        let r: Vec<_> = p
            .values()
            .map(|p| Rect::from_min_size(*p, egui::vec2(W, H)))
            .collect();
        for i in 0..r.len() {
            for j in i + 1..r.len() {
                assert!(!r[i].intersects(r[j]));
            }
        }
    }
    #[test]
    fn unknown_bridge_has_no_fake_physical_link() {
        let mut s = fixture();
        s["vms"][0]["Hardware"]["net1"]["iface"] = json!("missing");
        let g = build(&s, "", false, false);
        assert!(g.nodes.iter().any(|n| n.id == "unknown:missing"));
        assert!(!g.edges.iter().any(|e| e.from == "iface:missing"));
    }
}

// Attach only live services to visible, running guests. Snapshot freshness is explicit.
pub fn add_services(g: &mut Graph, snapshots: &BTreeMap<String, Value>) {
    let mut guests: Vec<_> = g
        .nodes
        .iter()
        .filter(|n| n.kind == "虚拟机" && n.status == "运行中")
        .cloned()
        .collect();
    if snapshots.contains_key("host") {
        if let Some(host)=g.nodes.iter().find(|n|n.id=="host").cloned(){guests.push(host);}
    }
    for vm in guests {
        let Some(snapshot) = snapshots.get(vm.id.trim_start_matches("vm:")) else {
            continue;
        };
        let stale = snapshot["stale"].as_bool().unwrap_or(true);
        for r in array(&snapshot["data"]["services"])
            .iter()
            .filter(|r| r["running"] == true)
        {
            let kind = field(r, "kind");
            if !matches!(kind, "forward" | "socks") {
                continue;
            }
            let endpoint = format!("{}:{}", field(r, "ip"), r["port"]);
            let id = format!("service:{}:{kind}:{endpoint}", vm.id);
            let count = r["stats"]["active"].as_u64();
            let color = if stale { GRAY } else { PURPLE };
            let mut n = node(
                &id,
                &endpoint,
                if kind == "socks" {
                    "SOCKS5 代理"
                } else {
                    "TCP 转发"
                },
                if stale {
                    "旧快照 · 待刷新"
                } else {
                    "监听中（快照）"
                },
                3,
                color,
                vec![
                    format!(
                        "允许：{}",
                        if field(r, "allow").is_empty() {
                            "未提供"
                        } else if field(r, "allow")=="0.0.0.0/0" {
                            "全部 IPv4 来源"
                        } else {
                            field(r, "allow")
                        }
                    ),
                    count
                        .map(|v| format!("活跃连接：{v}"))
                        .unwrap_or("活跃连接：未提供".into()),
                    if kind == "forward" {
                        format!("目标：{}", field(r, "target"))
                    } else if !r["active_targets"].is_array() {
                        "当前目标：此服务未提供遥测".into()
                    } else {
                        "目标随实际连接显示".into()
                    },
                ],
                r.clone(),
            );
            n.lines.insert(2, format!("运行于：{}", vm.title));
            n.vm = if vm.id=="host" {Some(field(r,"vm").into())}else{vm.vm.clone()};
            n.service = Some(r.clone());
            g.nodes.push(n);
            g.edges.push(edge(&vm.id, &id, "监听入口", !stale, color));
            let targets: Vec<(String, bool)> = if kind == "forward" {
                vec![(field(r, "target").into(), false)]
            } else {
                array(&r["active_targets"])
                    .iter()
                    .filter(|t| t["connections"].as_u64().unwrap_or(0) > 0)
                    .map(|t| (field(t, "target").into(), true))
                    .collect()
            };
            for (target, live) in targets.into_iter().filter(|(t, _)| !t.is_empty()) {
                let tid = format!("target:{id}:{target}");
                let mut n = node(
                    &tid,
                    &target,
                    "目标服务",
                    if stale {
                        "旧快照"
                    } else if live {
                        "通信中（快照）"
                    } else {
                        "已配置"
                    },
                    4,
                    color,
                    vec![if live {
                        "已建立的上游连接".into()
                    } else {
                        "固定转发目标；不代表已连通".into()
                    }],
                    r.clone(),
                );
                n.lines.push(format!("由 {} 访问", vm.title));
                n.vm = if vm.id=="host" {Some(field(r,"vm").into())}else{vm.vm.clone()};
                n.service = Some(r.clone());
                g.nodes.push(n);
                g.edges.push(edge(
                    &id,
                    &tid,
                    if live { "实际连接" } else { "转发配置" },
                    live && !stale,
                    color,
                ));
            }
        }
    }
}

#[cfg(test)]
mod service_tests {
    use super::*;
    use serde_json::json;
    fn host() -> Value {
        json!({"vms":[{"ID":"a","Name":"A","State":"running"},{"ID":"b","Name":"B","State":"running"},{"ID":"c","Name":"C","State":"suspended"}]})
    }
    fn snapshot(stale: bool) -> Value {
        json!({"stale":stale,"data":{"services":[
            {"kind":"forward","ip":"10.0.0.1","port":8080,"target":"127.0.0.1:80","running":true,"stats":{"active":0}},
            {"kind":"socks","ip":"10.0.0.1","port":1080,"running":true,"active_targets":[{"target":"example.test:443","connections":2},{"target":"closed:80","connections":0}]},
            {"kind":"forward","ip":"10.0.0.1","port":9999,"target":"stopped:80","running":false}
        ]}})
    }
    #[test]
    fn live_services_are_namespaced_and_stopped_guests_excluded() {
        let mut g = build(&host(), "", false, false);
        add_services(
            &mut g,
            &[
                ("a".into(), snapshot(false)),
                ("b".into(), snapshot(false)),
                ("c".into(), snapshot(false)),
            ]
            .into(),
        );
        assert_eq!(g.nodes.iter().filter(|n| n.column == 3).count(), 4);
        assert_eq!(g.nodes.iter().filter(|n| n.column == 4).count(), 4);
        assert_eq!(
            g.nodes
                .iter()
                .map(|n| &n.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            g.nodes.len()
        );
        assert!(!g
            .nodes
            .iter()
            .any(|n| n.title.contains("stopped") || n.title.contains("closed")));
        assert!(g
            .nodes
            .iter()
            .filter(|n| n.column >= 3)
            .all(|n| n.service.is_some() && n.vm.is_some()));
        let boxes: Vec<_> = g
            .positions()
            .values()
            .map(|p| Rect::from_min_size(*p, egui::vec2(W, H)))
            .collect();
        for i in 0..boxes.len() {
            for j in i + 1..boxes.len() {
                assert!(!boxes[i].intersects(boxes[j]));
            }
        }
    }
    #[test]
    fn stale_connections_are_not_presented_as_current() {
        let mut g = build(&host(), "A", false, false);
        add_services(&mut g, &[("a".into(), snapshot(true))].into());
        assert!(g
            .nodes
            .iter()
            .filter(|n| n.column >= 3)
            .all(|n| n.status.contains("旧快照")));
        assert!(g
            .edges
            .iter()
            .filter(|e| e.to.starts_with("target:") || e.to.starts_with("service:"))
            .all(|e| !e.connected));
    }
    #[test]
    fn mac_listener_is_distinct_and_loopback_belongs_only_to_mac() {
        let host=host();let mut g=build(&host,"",false,false);
        add_services(&mut g,&[("host".into(),json!({"stale":false,"data":{"services":[{"location":"host","kind":"forward","ip":"127.0.0.1","port":18787,"target":"10.211.55.18:8787","allow":"127.0.0.0/8","running":true,"vm":"A"}]}}))].into());
        let id="service:host:forward:127.0.0.1:18787";
        assert!(g.edges.iter().any(|e|e.from=="host"&&e.to==id));
        let access=access_graph(&g,&host,id);
        assert!(access.edges.iter().any(|e|e.from=="host"&&e.to==id&&e.label=="规则允许"));
        assert!(access.edges.iter().filter(|e|e.from.starts_with("vm:")).all(|e|e.label!="规则允许"));
        assert!(access.nodes.iter().find(|n|n.id==id).unwrap().lines.iter().any(|l|l.contains("运行于：本机 Mac")));
    }
    #[test]
    fn old_socks_logs_do_not_invent_active_targets() {
        let mut g = build(&host(), "A", false, false);
        add_services(&mut g,&[("a".into(),json!({"stale":false,"data":{"services":[{"kind":"socks","ip":"10.0.0.1","port":1080,"running":true,"events":[{"event":"connected","target":"old:80"}]}]}}))].into());
        assert_eq!(g.nodes.iter().filter(|n| n.column == 3).count(), 1);
        assert!(!g.nodes.iter().any(|n| n.column == 4));
    }
}

/// Match the union of IPv4 CIDRs; unmatched symbolic scopes stay unknown.
pub fn allowed_source(ip: &str, allow: &str) -> Option<bool> {
    let mut unknown = false;
    for scope in allow.split(',') {
        match allowed_single_source(ip, scope.trim()) {
            Some(true) => return Some(true),
            None => unknown = true,
            _ => (),
        }
    }
    if unknown { None } else { Some(false) }
}
fn allowed_single_source(ip: &str, allow: &str) -> Option<bool> {
    let ip = ip.parse::<std::net::Ipv4Addr>().ok()?;
    let (net, bits) = allow.split_once('/')?;
    let net = net.parse::<std::net::Ipv4Addr>().ok()?;
    let bits = bits.parse::<u32>().ok()?;
    if bits > 32 {
        return None;
    }
    let mask = if bits == 0 {
        0
    } else {
        u32::MAX << (32 - bits)
    };
    Some(u32::from(ip) & mask == u32::from(net) & mask)
}
pub fn access_clients(host: &Value) -> Vec<(String, String, Vec<String>)> {
    let mut result = vec![(
        "host".into(),
        "宿主机 Mac".into(),
        std::iter::once("127.0.0.1".to_string()).chain(array(&host["interfaces"])
            .iter()
            .flat_map(|i| array(&i["ips"]))
            .filter_map(|ip| ip.as_str().map(str::to_string)))
            .collect(),
    )];
    for v in array(&host["vms"])
        .iter()
        .filter(|v| field(v, "State") == "running")
    {
        result.push((
            format!("vm:{}", field(v, "ID")),
            field(v, "Name").into(),
            array(&v["Network"]["ipAddresses"])
                .iter()
                .filter(|a| field(a, "type") == "ipv4")
                .map(|a| field(a, "ip").into())
                .collect(),
        ));
    }
    result
}
pub fn access_status(ips: &[String], allow: &str) -> (String, Vec<String>) {
    let matching = ips
        .iter()
        .filter(|ip| allowed_source(ip, allow) == Some(true))
        .cloned()
        .collect::<Vec<_>>();
    let status = if !matching.is_empty() {
        "源 IP 符合规则"
    } else if ips.is_empty() || ips.iter().any(|ip| allowed_source(ip, allow).is_none()) {
        "待确认"
    } else {
        "已知 IP 未允许"
    };
    (status.into(), matching)
}
pub fn access_graph(physical: &Graph, host: &Value, selected: &str) -> Graph {
    let focus = physical
        .nodes
        .iter()
        .find(|n| n.id == selected)
        .filter(|n| n.service.is_some())
        .and_then(|n| n.service.as_ref().map(|r| (n.vm.clone(), r.clone())));
    let mut g = Graph::default();
    for n in physical.nodes.iter().filter(|n| n.column >= 3) {
        if let Some((vm, row)) = &focus {
            if &n.vm != vm
                || n.service
                    .as_ref()
                    .map(|r| (field(r,"location"), field(r, "kind"), field(r, "ip"), &r["port"]))
                    != Some((field(row,"location"), field(row, "kind"), field(row, "ip"), &row["port"]))
            {
                continue;
            }
        }
        let mut n = n.clone();
        n.column -= 2;
        g.nodes.push(n);
    }
    g.edges = physical
        .edges
        .iter()
        .filter(|e| g.nodes.iter().any(|n| n.id == e.from) && g.nodes.iter().any(|n| n.id == e.to))
        .cloned()
        .collect();
    let services = g
        .nodes
        .iter()
        .filter(|n| n.column == 1)
        .cloned()
        .collect::<Vec<_>>();
    for (id, title, ips) in access_clients(host) {
        let mut lines = vec![];
        let mut any = false;
        for service in &services {
            let r = service.service.as_ref().unwrap();
            let (status, matching) = access_status(&ips, field(r, "allow"));
            any |= !matching.is_empty();
            lines.push(format!("{}：{}", service.title, status));
            // Policy edges are dashed: a matching ACL is not a connectivity test.
            let mut e = edge(
                &id,
                &service.id,
                &status,
                false,
                if matching.is_empty() { GRAY } else { TEAL },
            );
            e.label = if service.status.contains("旧快照") {
                format!("旧规则：{status}")
            } else {
                if matching.is_empty() {
                    format!("规则：{status}")
                } else {
                    "规则允许".into()
                }
            };
            g.edges.push(e);
        }
        if services.is_empty() {
            continue;
        }
        let mut n = node(
            &id,
            &title,
            "访问来源",
            if any {
                "规则有匹配"
            } else {
                "未允许 / 待确认"
            },
            0,
            if any { TEAL } else { GRAY },
            vec![ips.join(", "), lines.join(" / ")],
            serde_json::json!({"已知源IP":ips,"规则判断":lines,"说明":"按已知源 IP 与服务 ACL 比较；实际源地址取决于路由 / NAT，仍需连通测试。"}),
        );
        n.vm = None;
        g.nodes.push(n);
    }
    g
}

#[cfg(test)]
mod access_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn exact_source_scope_and_unknown_are_distinct() {
        assert_eq!(allowed_source("10.211.55.2", "10.211.55.2/32"), Some(true));
        assert_eq!(
            allowed_source("10.211.55.18", "10.211.55.2/32"),
            Some(false)
        );
        assert_eq!(allowed_source("10.211.55.18", "10.211.55.0/24"), Some(true));
        assert_eq!(
            allowed_source("10.211.56.18", "10.211.55.0/24"),
            Some(false)
        );
        for scope in ["", "Any", "LocalSubnet", "10.0.0.1/99"] {
            assert_eq!(allowed_source("10.0.0.1", scope), None);
        }
        assert_eq!(access_status(&[], "10.0.0.1/32").0, "待确认");
    }
    #[test]
    fn multi_source_and_all_match_without_broadening_selection() {
        let allow="10.211.55.2/32,10.211.55.18/32";
        assert_eq!(allowed_source("10.211.55.2",allow),Some(true));
        assert_eq!(allowed_source("10.211.55.18",allow),Some(true));
        assert_eq!(allowed_source("10.211.55.19",allow),Some(false));
        assert_eq!(allowed_source("203.0.113.9","0.0.0.0/0"),Some(true));
        assert_eq!(allowed_source("10.211.55.19","10.211.55.2/32,LocalSubnet"),None);
        assert_eq!(allowed_source("10.211.55.2","10.211.55.2/32,LocalSubnet"),Some(true));
    }
    #[test]
    fn access_diagram_points_clients_to_listener_not_directly_to_target() {
        let host = json!({"interfaces":[{"ips":["10.211.55.2"]}],"vms":[{"ID":"a","Name":"Server","State":"running","Network":{"ipAddresses":[{"type":"ipv4","ip":"10.211.55.19"}]}},{"ID":"b","Name":"Kali","State":"running","Network":{"ipAddresses":[{"type":"ipv4","ip":"10.211.55.18"}]}}]});
        let mut g = build(&host, "", false, false);
        add_services(&mut g,&[("a".into(),json!({"stale":false,"data":{"services":[{"kind":"forward","ip":"10.211.55.19","port":8080,"target":"127.0.0.1:80","allow":"10.211.55.2/32","running":true}]}}))].into());
        let a = access_graph(&g, &host, "");
        assert!(a
            .edges
            .iter()
            .any(|e| e.from == "host" && e.to.starts_with("service:") && e.label == "规则允许"));
        assert!(a
            .edges
            .iter()
            .any(|e| e.from == "vm:b" && e.label.contains("未允许")));
        assert!(!a
            .edges
            .iter()
            .any(|e| e.from == "host" && e.to.starts_with("target:")));
        assert!(a
            .edges
            .iter()
            .filter(|e| e.to.starts_with("service:"))
            .all(|e| !e.connected));
    }
}
