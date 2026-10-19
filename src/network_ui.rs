impl App {
    fn sync_selection(&mut self) {
        if self.selection==self.vm{return;}
        self.selection=self.vm.clone();self.services=ServicesView::default();self.routes=RoutesView::default();self.pending=None;self.pending_host=None;
        self.containers.clear();self.address_rows.clear();self.listen_ip.clear();self.ip_search.clear();
        self.net_device.clear();self.output=format!("当前虚拟机：{}。请选择操作。",self.vm);
        if let Some(g)=self.vms.iter().find(|g|g.name==self.vm) {
            if g.ips.len()==1 {self.listen_ip=g.ips[0].clone();}
        }
    }
    fn ip_picker(&mut self,ui:&mut egui::Ui) {
        input_field(ui,"虚拟机监听 IP",&mut self.listen_ip,"选择或输入虚拟机网卡 IPv4");
        let mut candidates:Vec<(String,String)>=self.address_rows.iter().filter_map(|r|{
            let ip=hostnet::field(r,"ip");
            ip.parse::<std::net::Ipv4Addr>().ok().filter(|i|i.is_private()).map(|_|(ip.to_string(),format!("{} · {} · {}",ip,hostnet::field(r,"interface"),hostnet::field(r,"state"))))
        }).collect();
        if candidates.is_empty() {if let Some(g)=self.vms.iter().find(|g|g.name==self.vm){
            candidates=g.ips.iter().map(|ip|(ip.clone(),format!("{ip} · PD 报告"))).collect();
        }}
        candidates.sort();candidates.dedup_by(|a,b|a.0==b.0);
        ui.horizontal(|ui|{
            egui::ComboBox::from_id_salt("listen-picker").close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).selected_text("选择网卡 IP").width((ui.available_width()-96.).max(90.)).show_ui(ui,|ui|{
                ui.add(egui::TextEdit::singleline(&mut self.ip_search).hint_text("搜索 IP 或网卡名"));
                for (ip,label) in &candidates {
                    if label.to_lowercase().contains(&self.ip_search.to_lowercase())&&ui.selectable_value(&mut self.listen_ip,ip.clone(),label).changed(){self.pending=None;ui.memory_mut(|m|m.close_popup());}
                }
                if candidates.is_empty(){ui.label("暂无地址，请读取网卡 IP 或手动填写。");}
            });
            if ui.add_enabled(self.rx.is_none(),egui::Button::new("读取 IP")).clicked(){self.launch(self.base(&["addresses"]),"addresses");}
        });
        ui.label(RichText::new("PD 地址可能是缓存，可读取网卡 IP 核对。").size(11.).color(MUTED));
    }
    fn host_reply(&mut self,r:Reply){
        self.good=r.ok;
        self.message=if r.ok{"完成"}else{"操作失败，请查看详情"}.into();
        if !r.ok {if r.purpose=="host-read"{self.topology.error=r.text.clone();}self.output=r.text;self.pending_host=None;return;}
        let parsed=serde_json::from_str::<serde_json::Value>(&r.text);
        let Ok(value)=parsed else{self.good=false;self.output=format!("无法解析网络管理响应：{}",r.text);return;};
        match r.purpose.as_str(){
            "host-preview"=>{
                let id=self.vms.iter().find(|g|g.name==self.vm).map(|g|g.id.as_str()).unwrap_or("");
                if hostnet::field(&value["request"],"vm")!=id {return;}
                self.output=hostnet::field(&value,"description").into();self.pending_host=Some(value);
            }
            "host-read"|"host-apply"=>{
                self.topology_services.refresh=true;
                self.topology.loaded=Some(std::time::Instant::now());self.topology.error.clear();
                self.pending_host=None;
                self.output=if r.purpose=="host-apply" {hostnet::field(&value,"result").into()}else{"已读取本机接口、PD 网络、USB 设备与虚拟网卡。".into()};
                self.host=if r.purpose=="host-apply"{value["snapshot"].clone()}else{value};
                self.vms=hostnet::array(&self.host["vms"]).iter().map(guest::Guest::parse).collect();
            }
            _=>{}
        }
        self.log.push(format!("{} · 完成",r.purpose));
    }
    fn network_request(&mut self,op:&str){
        let Some(g)=self.vms.iter().find(|g|g.name==self.vm) else{return;};
        let req=serde_json::json!({"vm":g.id,"op":op,"device":if op.starts_with("usb-"){&self.usb_device}else{&self.net_device},"mode":self.net_mode,"iface":self.bridge_iface});
        self.pending_host=None;self.pending=None;self.launch(vec![req.to_string()],"host-preview");
    }
    fn network_preview_ui(&mut self,ui:&mut egui::Ui){
        if let Some(plan)=self.pending_host.clone(){
            egui::Frame::group(ui.style()).fill(Color32::from_rgb(237,243,255)).show(ui,|ui|{
                ui.strong("网络变更预览");ui.label(hostnet::field(&plan,"description"));
                let argv=hostnet::array(&plan["args"]).iter().map(|v|format!("{:?}",v.as_str().unwrap_or(""))).collect::<Vec<_>>().join(" ");
                ui.monospace(format!("{} {}",hostnet::field(&plan,"bin"),argv));
                ui.horizontal(|ui|{
                    if ui.add_enabled(self.rx.is_none(),egui::Button::new("应用这份网络配置")).clicked(){self.pending_host=None;self.launch(vec![plan.to_string()],"host-apply");}
                    if ui.button("取消").clicked(){self.pending_host=None;}
                });
            });
        }
    }
    fn network_ui(&mut self,ui:&mut egui::Ui){
        ui.add_enabled_ui(self.rx.is_none(),|ui|{
            ui.horizontal(|ui|{ui.strong("Mac 网卡与设备");if ui.button("刷新网络 / USB").clicked(){self.pending_host=None;self.launch(vec![],"host-read");}});
            for error in hostnet::array(&self.host["errors"]){ui.colored_label(Color32::DARK_RED,error.as_str().unwrap_or("读取失败"));}
            egui::Grid::new("host-interfaces").striped(true).show(ui,|ui|{
                for h in ["Mac 接口","设备名称","链路","IPv4","桥接到的虚拟机"]{ui.strong(h);}ui.end_row();
                for iface in hostnet::array(&self.host["interfaces"]){
                    let dev=hostnet::field(iface,"device");
                    let linked=hostnet::array(&self.host["vms"]).iter().flat_map(|v|hostnet::adapters(v).into_iter().filter(|(_,a)|hostnet::field(a,"type")=="bridged"&&(hostnet::bridge_interface(&self.host,a)==dev||hostnet::field(a,"bound-iface")==dev)).map(move |(n,_)|format!("{} / {n}",hostnet::field(v,"Name")))).collect::<Vec<_>>().join(", ");
                    ui.label(dev);ui.label(hostnet::field(iface,"name"));ui.label(match hostnet::field(iface,"state"){"active"=>"已连接","inactive"=>"未连接",_=>"—"});
                    ui.label(hostnet::array(&iface["ips"]).iter().filter_map(|v|v.as_str()).collect::<Vec<_>>().join(", "));ui.label(if linked.is_empty(){"—"}else{&linked});ui.end_row();
                }
            });
            ui.small("桥接：网卡留在 Mac，虚拟机通过它入网。USB 直通：设备由单台虚拟机独占，Mac 无法同时使用。");
            ui.separator();
            let selected=self.vms.iter().find(|g|g.name==self.vm).cloned();
            let vm=selected.as_ref().and_then(|g|hostnet::array(&self.host["vms"]).iter().find(|v|hostnet::field(v,"ID")==g.id)).cloned().unwrap_or_default();
            let adapters=hostnet::adapters(&vm);
            ui.strong("虚拟机网卡与网络模式");
            for (name,a) in &adapters {
                ui.label(format!("{}：{} · {} · {} · {}",name,mode_label(hostnet::field(a,"type")),hostnet::field(a,"iface"),if a["enabled"]==false{"已禁用"}else{"已启用"},if hostnet::field(a,"state")=="disconnected"{"网线断开"}else{"网线连接"}));
            }
            if self.net_device.is_empty(){if let Some((dev,a))=adapters.first(){self.net_device=dev.clone();self.net_mode=hostnet::field(a,"type").into();self.bridge_iface=hostnet::bridge_interface(&self.host,a);}}
            ui.add_enabled_ui(selected.is_some(),|ui|{
                ui.horizontal(|ui|{
                    ui.label("要修改的网卡");
                    egui::ComboBox::from_id_salt("net-device").selected_text(if self.net_device.is_empty(){"请选择"}else{&self.net_device}).show_ui(ui,|ui|{
                        for (dev,a) in &adapters {if ui.selectable_value(&mut self.net_device,dev.clone(),dev).changed(){self.net_mode=hostnet::field(a,"type").into();self.bridge_iface=hostnet::bridge_interface(&self.host,a);self.pending_host=None;}}
                    });
                    egui::ComboBox::from_id_salt("net-mode").selected_text(mode_label(&self.net_mode)).show_ui(ui,|ui|{
                        for mode in ["shared","bridged","host-only"]{if ui.selectable_value(&mut self.net_mode,mode.into(),mode_label(mode)).changed(){self.pending_host=None;}}
                    });
                });
                if self.net_mode=="bridged" {
                    ui.horizontal(|ui|{ui.label("桥接到 Mac 网卡");egui::ComboBox::from_id_salt("bridge-interface").selected_text(if self.bridge_iface.is_empty(){"请选择物理接口"}else{&self.bridge_iface}).show_ui(ui,|ui|{
                        for n in hostnet::array(&self.host["networks"]){let iface=hostnet::field(n,"Bound To");if hostnet::field(n,"Type")=="bridged"&&iface!="FF:FF:FF:FF:FF:FF"&&!iface.is_empty(){if ui.selectable_value(&mut self.bridge_iface,iface.into(),format!("{} · {}",hostnet::field(n,"Network ID"),iface)).changed(){self.pending_host=None;}}}
                    });});
                    ui.small("USB 网卡插入并归属 Mac 后，刷新即可选择。PD 未报告可桥接的接口不会出现在这里。");
                }
                ui.label(match self.net_mode.as_str(){"shared"=>"共享（NAT）：使用 Mac 当前的网络出口。","bridged"=>"桥接：虚拟机通过指定物理网卡接入其网络，IP 由该网络或来宾配置决定。",_=>"仅主机：用于 Mac 与虚拟机之间的隔离网络，默认不提供外网。"});
                ui.horizontal_wrapped(|ui|{
                    if ui.add_enabled(!self.net_device.is_empty(),egui::Button::new("预览修改模式")).clicked(){self.network_request("mode");}
                    if ui.button("预览新增网卡").clicked(){self.network_request("add");}
                    if ui.add_enabled(!self.net_device.is_empty(),egui::Button::new("连接网线")).clicked(){self.network_request("connect");}
                    if ui.add_enabled(!self.net_device.is_empty(),egui::Button::new("断开网线")).clicked(){self.network_request("disconnect");}
                });
                ui.horizontal(|ui|{for (label,op) in [("启用网卡","enable"),("禁用网卡","disable")]{if ui.add_enabled(!self.net_device.is_empty(),egui::Button::new(label)).clicked(){self.network_request(op);}}});
                ui.small("双网用法：保留共享网卡上外网，新增桥接到 USB 有线网卡的网卡上内网。来宾 IP、网关与路由仍需按实际网络配置。");
            });
            ui.separator();ui.strong("USB 网卡归属 / 独占直通");
            ui.horizontal(|ui|{
                ui.add(egui::TextEdit::singleline(&mut self.usb_search).hint_text("搜索 USB 设备名称").desired_width(160.));
                egui::ComboBox::from_id_salt("usb-device").width(310.).selected_text(hostnet::array(&self.host["usb"]).iter().find(|u|hostnet::field(u,"System name")==self.usb_device).map(|u|hostnet::field(u,"Name")).unwrap_or("选择 USB 网卡 / 设备")).show_ui(ui,|ui|{
                    for u in hostnet::array(&self.host["usb"]){let name=hostnet::field(u,"Name");if name.to_lowercase().contains(&self.usb_search.to_lowercase()){
                        let state=if hostnet::field(u,"Connected-To-Vm")!="NO" {"已连接虚拟机"}else if hostnet::field(u,"Present-On-Host")=="YES" {"已插入"}else{"未插入"};
                        if ui.selectable_value(&mut self.usb_device,hostnet::field(u,"System name").into(),format!("{name} · {state}")).changed(){self.pending_host=None;}
                    }}
                });
            });
            if let Some(u)=hostnet::array(&self.host["usb"]).iter().find(|u|hostnet::field(u,"System name")==self.usb_device){
                ui.label(format!("当前连接：{} · 插入 Mac：{} · 插入时行为：{} · 指定 VM：{}",hostnet::field(u,"Connected-To-Vm"),hostnet::field(u,"Present-On-Host"),hostnet::field(u,"Autoconnect-Action"),hostnet::field(u,"Autoconnect-Vm-Uuid")));
            }
            ui.add_enabled_ui(selected.is_some()&&!self.usb_device.is_empty(),|ui|{
                ui.horizontal_wrapped(|ui|{for (label,op) in [("预览连接到所选 VM","usb-connect"),("预览从所选 VM 释放","usb-disconnect"),("记住归属到所选 VM","usb-assign"),("下次插入归属 Mac","usb-host"),("下次插入询问","usb-ask")]{if ui.button(label).clicked(){self.network_request(op);}}});
            });
            ui.small("即时操作要求设备已插入且 VM 运行中。释放后如配置了自动分配，需把下次归属改为 Mac。PD / 来宾驱动决定设备兼容性。");
        });
    }
}
fn mode_label(mode:&str)->&str{match mode{"shared"=>"共享网络（NAT）","bridged"=>"桥接网络","host"|"host-only"=>"仅主机网络",_=>mode}}
