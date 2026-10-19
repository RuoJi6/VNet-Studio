#[derive(Default)]
struct RoutesView {
    data: serde_json::Value,
    plan: Option<serde_json::Value>,
    interface: String,
    gateway: String,
    destination: String,
    lookup: String,
    result: String,
    error: String,
    undo_confirm: bool,
    loaded: Option<std::time::Instant>,
}
impl App {
    fn route_launch(&mut self, action:&str, request:serde_json::Value) {
        let mut args=self.base(&["routes",action]);
        if !matches!(action,"list"|"rollback") {args.extend(["--request".into(),request.to_string()]);}
        self.routes.plan=None;self.routes.error.clear();self.routes.undo_confirm=false;
        self.launch(args,&format!("routes-{action}"));
    }
    fn routes_reply(&mut self,r:Reply) {
        self.good=r.ok;
        if !r.ok {self.routes.error=r.text.clone();self.output=r.text;self.message="路由操作失败，请查看原因".into();return;}
        let Ok(v)=serde_json::from_str::<serde_json::Value>(&r.text) else {
            self.routes.error="无法解析路由响应".into();self.good=false;return;
        };
        match r.purpose.as_str() {
            "routes-list"=>{self.routes.data=v;self.routes.loaded=Some(std::time::Instant::now());},
            "routes-preview"=>self.routes.plan=Some(v),
            "routes-lookup"=>self.routes.result=hostnet::field(&v,"result").into(),
            "routes-apply"|"routes-rollback"=>{
                self.routes.data=v["snapshot"].clone();self.routes.plan=None;self.routes.loaded=Some(std::time::Instant::now());
                self.routes.result=hostnet::field(&v,"result").into();
            },
            _=>{}
        }
        self.routes.error.clear();self.output=r.text;self.message="完成 · 路由操作已核对".into();
    }
    fn route_preview(&mut self, op:&str) {
        let request=serde_json::json!({"op":op,"interface":self.routes.interface,"gateway":self.routes.gateway.trim(),"destination":self.routes.destination.trim()});
        self.route_launch("preview",request);
    }
    fn routes_ui(&mut self,ui:&mut egui::Ui) {
        if self.routes.data.is_null()&&self.routes.error.is_empty()&&self.rx.is_none(){self.route_launch("list",serde_json::json!({}));}
        ui.strong(format!("{} · IPv4 出口与路由",self.vm));
        ui.label("默认出口用于没有更具体路由的目标；指定 IP / 网段按最长前缀匹配优先。");
        ui.horizontal_wrapped(|ui|{
            if ui.add_enabled(self.rx.is_none(),egui::Button::new("读取网卡与路由")).clicked(){self.route_launch("list",serde_json::json!({}));}
            if let Some(t)=self.routes.loaded{ui.label(format!("{} 秒前读取",t.elapsed().as_secs()));}
            if self.rx.is_some(){ui.spinner();}
        });
        if !self.routes.error.is_empty(){ui.colored_label(Color32::DARK_RED,&self.routes.error);}
        if self.routes.data.is_null(){return;}
        ui.small("当前会话生效。不会修改 DNS、IPv6、持久化网络配置；DHCP / VPN 重连可能覆盖，请重新查询。需要来宾管理员权限。");
        for w in hostnet::array(&self.routes.data["warnings"]){ui.colored_label(Color32::from_rgb(165,103,24),w.as_str().unwrap_or(""));}
        let interfaces=hostnet::array(&self.routes.data["interfaces"]).to_vec();
        let rows=hostnet::array(&self.routes.data["routes"]).to_vec();
        egui::CollapsingHeader::new("当前默认路由").default_open(true).show(ui,|ui|{
            for r in rows.iter().filter(|r|hostnet::field(r,"destination")=="0.0.0.0/0") {
                let iface=interfaces.iter().find(|i|hostnet::field(i,"id")==hostnet::field(r,"interface"));
                ui.label(format!("{} → 网关 {} · route metric {}{}",iface.map(|i|hostnet::field(i,"name")).unwrap_or(hostnet::field(r,"interface")),hostnet::field(r,"gateway"),r["metric"],if hostnet::field(r,"flags").contains('I'){" · 接口作用域"}else{""}));
            }
        });
        ui.separator();
        ui.add_enabled_ui(self.rx.is_none(),|ui|{
            let mut changed=false;
            ui.horizontal_wrapped(|ui|{
                ui.strong("出口网卡");
                egui::ComboBox::from_id_salt("route-interface").selected_text(interfaces.iter().find(|i|hostnet::field(i,"id")==self.routes.interface).map(|i|hostnet::field(i,"name")).unwrap_or("选择来宾网卡")).width(350.).show_ui(ui,|ui|{
                    for i in &interfaces {
                        if hostnet::array(&i["ips"]).is_empty()||matches!(hostnet::field(i,"id"),"lo"|"lo0"){continue;}
                        let label=format!("{} · {} · {}",hostnet::field(i,"name"),hostnet::field(i,"state"),hostnet::array(&i["ips"]).iter().filter_map(|v|v.as_str()).collect::<Vec<_>>().join(", "));
                        if ui.selectable_value(&mut self.routes.interface,hostnet::field(i,"id").into(),label).changed(){
                            self.routes.gateway=rows.iter().find(|r|hostnet::field(r,"interface")==self.routes.interface&&hostnet::field(r,"destination")=="0.0.0.0/0").map(|r|hostnet::field(r,"gateway").to_string()).unwrap_or_default();changed=true;
                        }
                    }
                });
            });
            ui.label("下一跳网关（直连目标可留空；默认出口必须填写）");
            changed|=ui.add(egui::TextEdit::singleline(&mut self.routes.gateway).desired_width(360.).hint_text("例如 10.211.55.1，必须能由所选网卡直连")).changed();
            if ui.add_enabled(!self.routes.interface.is_empty(),egui::Button::new("预览设为默认 IPv4 出口")).clicked(){self.route_preview("default");}
            ui.separator();ui.strong("指定 IP / 网段走所选网卡");
            changed|=ui.add(egui::TextEdit::singleline(&mut self.routes.destination).desired_width(360.).hint_text("例如 192.168.10.117 或 192.168.10.0/24")).changed();
            ui.horizontal_wrapped(|ui|{
                if ui.add_enabled(!self.routes.interface.is_empty()&&!self.routes.destination.trim().is_empty(),egui::Button::new("预览添加指定路由")).clicked(){self.route_preview("add");}
                if ui.add_enabled(!self.routes.destination.trim().is_empty(),egui::Button::new("预览删除该网段路由")).clicked(){self.route_preview("delete");}
            });
            if changed{self.routes.plan=None;self.routes.undo_confirm=false;}
        });
        if let Some(p)=self.routes.plan.clone(){
            egui::Frame::group(ui.style()).fill(Color32::from_rgb(237,243,255)).show(ui,|ui|{
                ui.strong(hostnet::field(&p,"description"));
                ui.label(hostnet::field(&p,"note"));
                for (key,label) in [("before","移除"),("after","新增")] {
                    for r in hostnet::array(&p[key]){ui.monospace(format!("{} {} → {} via {} · metric {}",label,hostnet::field(r,"destination"),hostnet::field(r,"interface"),if hostnet::field(r,"gateway").is_empty(){"直连"}else{hostnet::field(r,"gateway")},r["metric"]));}
                }
                ui.horizontal(|ui|{
                    if ui.add_enabled(self.rx.is_none(),egui::Button::new("应用这份路由配置")).clicked(){self.route_launch("apply",p);}
                    if ui.button("取消").clicked(){self.routes.plan=None;}
                });
            });
        }
        ui.separator();ui.strong("验证目标实际出口（不发送探测包）");
        ui.horizontal_wrapped(|ui|{
            ui.add(egui::TextEdit::singleline(&mut self.routes.lookup).desired_width(270.).hint_text("填写 IPv4，例如 192.168.10.117"));
            if ui.add_enabled(self.rx.is_none()&&!self.routes.lookup.trim().is_empty(),egui::Button::new("查询出口网卡")).clicked(){self.route_launch("lookup",serde_json::json!({"ip":self.routes.lookup.trim()}));}
        });
        if !self.routes.result.is_empty(){egui::ScrollArea::both().max_height(160.).id_salt("route-result").show(ui,|ui|{ui.monospace(&self.routes.result);});}
        if self.routes.data["can_rollback"]==true {
            if ui.add_enabled(self.rx.is_none(),egui::Button::new("撤销上次路由变更…")).clicked(){self.routes.undo_confirm=true;self.routes.plan=None;}
            if self.routes.undo_confirm {ui.horizontal_wrapped(|ui|{
                ui.label("恢复上次操作涉及的路由；若已被其他程序修改，会拒绝撤销。");
                if ui.add_enabled(self.rx.is_none(),egui::Button::new("确认撤销上次变更")).clicked(){self.route_launch("rollback",serde_json::json!({}));}
                if ui.button("取消撤销").clicked(){self.routes.undo_confirm=false;}
            });}
        }
        egui::CollapsingHeader::new("完整 IPv4 路由表").show(ui,|ui|{
            egui::ScrollArea::both().max_height(210.).show(ui,|ui|{
                for r in &rows{ui.monospace(format!("{}  via {}  dev {}  metric {}  {}{}",hostnet::field(r,"destination"),hostnet::field(r,"gateway"),hostnet::field(r,"interface"),r["metric"],hostnet::field(r,"protocol"),if r["protected"]==true{" · 系统/复杂路由只读"}else{""}));}
            });
        });
        egui::CollapsingHeader::new("DNS 配置（只读）").show(ui,|ui|{ui.monospace(hostnet::field(&self.routes.data,"dns"));});
    }
}
