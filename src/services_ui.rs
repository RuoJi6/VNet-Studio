#[derive(Default)]
struct ServicesView {
    data: serde_json::Value,
    result: String,
    batch_text: String,
    batch_mode: bool,
    access_mode: String,
    source_search: String,
    selected: String,
    query: String,
    error: String,
    auto: bool,
    refresh: bool,
    loaded: Option<std::time::Instant>,
    attempt: Option<std::time::Instant>,
}
fn service_id(r:&serde_json::Value)->String {format!("{}{}:{}:{}",if r["location"]=="host"{"host:"}else{""},hostnet::field(r,"kind"),hostnet::field(r,"ip"),r["port"])}
impl App {
    fn access_picker(&mut self,ui:&mut egui::Ui){
        ui.strong("允许谁使用此入口");
        if self.services.access_mode.is_empty() {
            self.services.access_mode=if self.allow=="0.0.0.0/0"{"all"}else if self.allow.contains(','){"multi"}else{"single"}.into();
        }
        let previous=self.services.access_mode.clone();
        ui.horizontal_wrapped(|ui|{
            for (mode,label) in [("single","单选主机"),("multi","多选主机"),("all","全部来源"),("custom","自定义 IP / 网段")] {
                ui.radio_value(&mut self.services.access_mode,mode.into(),label);
            }
            if ui.button("刷新来源列表").clicked(){self.launch(vec![],"host-read");}
        });
        if previous!=self.services.access_mode {
            if self.services.access_mode=="all" {self.allow="0.0.0.0/0".into();}
            else if self.allow=="0.0.0.0/0" || self.services.access_mode=="single" {self.allow.clear();}
            self.pending=None;
        }
        if self.services.access_mode=="all" {
            ui.label("所有能够连接此监听地址的 IPv4 主机均可使用，包括宿主机、其他虚拟机及可达的局域网主机。");
            ui.small("不改变监听网卡、路由或其他防火墙规则；不会自动建立公网映射。");
        } else if self.services.access_mode=="custom" {
            ui.label("用英文逗号分隔多个 IPv4/CIDR；单个 IP 使用 /32");
            if ui.add(egui::TextEdit::singleline(&mut self.allow).desired_width(600.).hint_text("10.211.55.2/32,10.211.55.18/32")).changed(){self.pending=None;}
        } else {
            ui.add(egui::TextEdit::singleline(&mut self.services.source_search).desired_width(360.).hint_text("搜索宿主机 / 虚拟机名称 / IP"));
            let clients=topology::access_clients(&self.host);
            let mut visible=0;
            egui::ScrollArea::vertical().id_salt("access-source-list").max_height(155.).show(ui,|ui|{
                for (id,name,ips) in clients {
                    let mut scopes=ips.iter().filter_map(|ip|ip.parse::<std::net::Ipv4Addr>().ok().filter(|a|a.is_private()).map(|a|format!("{a}/32"))).collect::<Vec<_>>();
                    scopes.sort();scopes.dedup();
                    let label=format!("{} · {}",name,if scopes.is_empty(){"暂无可用 IPv4".into()}else{scopes.join(", ")});
                    if !label.to_lowercase().contains(&self.services.source_search.to_lowercase()){continue;}
                    visible+=1;
                    ui.push_id(id,|ui|{
                        let mut selected=self.allow.split(',').any(|a|scopes.iter().any(|s|s==a.trim()));
                        let response=if self.services.access_mode=="multi" {
                            ui.add_enabled(!scopes.is_empty(),egui::Checkbox::new(&mut selected,label))
                        } else {ui.add_enabled(!scopes.is_empty(),egui::RadioButton::new(selected,label))};
                        if response.clicked() {
                            if self.services.access_mode=="single" {self.allow=scopes.join(",");}
                            else {
                                let mut chosen=self.allow.split(',').filter(|a|!a.trim().is_empty()&&!scopes.iter().any(|s|s==a.trim())).map(|s|s.trim().to_string()).collect::<Vec<_>>();
                                if selected {chosen.extend(scopes);}
                                chosen.sort();chosen.dedup();self.allow=chosen.join(",");
                            }
                            self.pending=None;
                        }
                    });
                }
            });
            if visible==0 || self.host.is_null(){ui.label("点击“刷新来源列表”读取宿主机和运行中虚拟机的 IP。");}
        }
        ui.label(format!("当前允许来源：{}",if self.allow.is_empty(){"尚未选择"}else if self.allow=="0.0.0.0/0"{"全部 IPv4 来源"}else{&self.allow}));
        ui.small("按实际连接的源 IP 判断；IP 变化后需更新规则。修改运行中的服务需先停止，再按新范围添加并开启。");
    }

    fn service_change(&mut self,row:&serde_json::Value,action:&str){
        if row["location"]=="host" {
            self.launch(vec!["host-forward".into(),action.into(),"--listen".into(),format!("{}:{}",hostnet::field(row,"ip"),row["port"])],if action=="check"{"local-check"}else{"local-service-change"});
            return;
        }
        if row["managed"]!=true {self.services.result="只能管理本工具的规则".into();return;}
        let kind=hostnet::field(row,"kind");
        let op=match action {"start"=>if kind=="forward"{"add"}else{"start"},"stop"=>if kind=="forward"{"remove"}else{"stop"},_=>"delete"};
        let mut args=self.base(&[kind,op]);args.extend(["--listen".into(),format!("{}:{}",hostnet::field(row,"ip"),row["port"])]);
        if action=="start" {
            let allow=hostnet::field(row,"allow");
            if allow.is_empty(){self.services.result="记录缺少允许来源；请在新增表单明确填写后开启。".into();return;}
            args.extend(["--allow".into(),allow.into()]);
            if kind=="forward"{args.extend(["--target".into(),hostnet::field(row,"target").into()]);}
            if kind=="socks" {if let Some(path)=row["gost"].as_str().filter(|p|!p.is_empty()){args.extend(["--gost".into(),path.into()]);}}
        }
        self.pending=None;self.launch(args,"service-change");
    }
    fn service_manager(&mut self,ui:&mut egui::Ui,page:Page){
        self.service_form(ui,page);ui.separator();
        self.services_ui_filtered(ui,if page==Page::Forward{"forward"}else{"socks"});
    }
    fn service_form(&mut self,ui:&mut egui::Ui,page:Page) {
        self.rule_form_fields(ui,page);ui.add_space(16.);ui.separator();self.rule_submit_ui(ui,page);
    }
    fn submit_service_rule(&mut self,page:Page) {
        let local=page==Page::Forward&&self.local_forward;
        let kind=if page==Page::Forward{"forward"}else{"socks"};
        let unix=self.vms.iter().find(|g|g.name==self.vm).map(|g|g.unix()).unwrap_or(false);
        if local {std::mem::swap(&mut self.port,&mut self.local_port);std::mem::swap(&mut self.target,&mut self.local_target);std::mem::swap(&mut self.services.batch_mode,&mut self.local_batch_mode);std::mem::swap(&mut self.services.batch_text,&mut self.local_batch_text);}
            let lines=if self.services.batch_mode {self.services.batch_text.lines().map(str::to_string).collect::<Vec<_>>()}else{vec![if page==Page::Forward{format!("{} {}",self.port.trim(),self.target.trim())}else{self.socks_port.trim().into()}]};
            let mut requests=vec![];let mut local_rules=vec![];let mut error=None;
            for (index,line) in lines.iter().enumerate(){
                let words=line.split_whitespace().collect::<Vec<_>>();if words.is_empty()||line.trim_start().starts_with('#'){continue}
                let min=if page==Page::Forward {2}else{1};
                if words.len()<min||words.len()>min+if local{0}else{1} {error=Some(format!("第 {} 行格式不正确",index+1));break;}
                if local {
                    let vm_id=self.vms.iter().find(|g|g.name==self.vm).map(|g|g.id.clone()).unwrap_or_default();
                    local_rules.push(serde_json::json!({"listen":format!("{}:{}",self.local_ip.trim(),words[0]),"target":words[1],"vm":self.vm,"vm_id":vm_id}));
                    continue;
                }
                let mut request=vec![kind.to_string(),if page==Page::Forward{"add"}else{"start"}.into(),"--listen".into(),self.endpoint(words[0]),"--allow".into(),words.get(min).copied().unwrap_or(self.allow.trim()).into()];
                if page==Page::Forward{request.extend(["--target".into(),words[1].into()]);}
                if !unix&&page==Page::Socks{request.extend(["--gost".into(),self.gost.clone()]);}
                requests.push(request);
            }
            if let Some(error)=error {if local{self.local_services.result=error;}else{self.services.result=error;}self.good=false;}
            else if local && !local_rules.is_empty(){
                self.launch(vec!["host-forward".into(),"batch".into(),"--request".into(),serde_json::to_string(&local_rules).unwrap()],"local-service-change");
            }
            else if requests.is_empty(){if local{self.local_services.result="请至少填写一条 Mac 规则".into();}else{self.services.result="请至少填写一条规则".into();}}
            else {let mut args=self.base(&["services","batch"]);args.extend(["--request".into(),serde_json::to_string(&requests).unwrap()]);self.pending=None;self.launch(args,"service-change");}

        if local {std::mem::swap(&mut self.port,&mut self.local_port);std::mem::swap(&mut self.target,&mut self.local_target);std::mem::swap(&mut self.services.batch_mode,&mut self.local_batch_mode);std::mem::swap(&mut self.services.batch_text,&mut self.local_batch_text);}
    }
    fn service_preview_ui(&mut self,ui:&mut egui::Ui) {
                if let Some(args)=self.pending.clone(){
                    egui::Frame::group(ui.style()).fill(Color32::from_rgb(237,243,255)).show(ui,|ui|{
                        ui.label(RichText::new("配置预览已生成，尚未修改虚拟机").strong());
                        ui.label(format!("将执行：{}",args.join(" ")));
                        ui.horizontal(|ui|{
                            if ui.add_enabled(self.rx.is_none(),egui::Button::new(RichText::new("应用这份预览").color(BLUE))).clicked(){self.pending=None;self.launch(args,"apply");}
                            if ui.button("取消").clicked(){self.pending=None;}
                        });
                    });
                }

    }
    fn open_topology_tool(&mut self, vm:&str, tool:Page) {
        self.vm=vm.into();self.sync_selection();self.pending=None;self.pending_host=None;
        self.topology_tool=Some(tool);
        if matches!(tool,Page::Services|Page::Forward|Page::Socks) {self.services.refresh=true;}
    }
    fn services_tick(&mut self,ctx:&egui::Context) {
        if !matches!(self.page,Page::Services|Page::Forward|Page::Socks) && !matches!(self.topology_tool,Some(Page::Services|Page::Forward|Page::Socks)) {return;}
        ctx.request_repaint_after(Duration::from_secs(1));
        let local_due=self.local_services.attempt.map(|t|t.elapsed()>=Duration::from_secs(5)).unwrap_or(true);
        if self.rx.is_none() && (self.local_services.refresh || self.local_services.attempt.is_none() || (self.local_services.auto && local_due)) {
            self.local_services.refresh=false;self.local_services.attempt=Some(std::time::Instant::now());
            self.launch(vec!["host-forward".into(),"list".into()],"local-services");
            return;
        }
        let ready=self.vms.iter().find(|g|g.name==self.vm).map(|g|g.check().is_ok()).unwrap_or(false);
        let due=self.services.attempt.map(|t|t.elapsed()>=Duration::from_secs(5)).unwrap_or(true);
        if ready && self.rx.is_none() && (self.services.refresh || self.services.attempt.is_none() || (self.services.auto && due)) {
            self.services.refresh=false;self.services.attempt=Some(std::time::Instant::now());
            self.launch(self.base(&["services","list"]),"services");
        }
    }
    fn topology_tool_ui(&mut self,ctx:&egui::Context) {
        let Some(mut tool)=self.topology_tool else{return;};
        let mut open=true;
        egui::Window::new(format!("拓扑操作 · {}",self.vm)).id(egui::Id::new("topology-tools"))
            .open(&mut open).default_width(820.).max_height(660.).resizable(true).vscroll(true).show(ctx,|ui|{
                ui.add_enabled_ui(self.rx.is_none(),|ui|{
                    ui.horizontal_wrapped(|ui|{
                        for (p,label) in [(Page::Host,"网络设置"),(Page::Routes,"默认出口 / 路由"),(Page::Forward,"转发规则"),(Page::Socks,"SOCKS5 代理"),(Page::Services,"运行与日志")] {
                            if ui.selectable_value(&mut tool,p,label).changed(){self.pending=None;self.pending_host=None;self.output.clear();if matches!(p,Page::Services|Page::Forward|Page::Socks){self.services.refresh=true;}}
                        }
                    });
                });
                ui.separator();
                if tool==Page::Host {self.network_ui(ui);self.network_preview_ui(ui);}
                else {
                    let block=self.vms.iter().find(|g|g.name==self.vm).and_then(|g|g.check().err());
                    if let Some(reason)=&block {ui.colored_label(Color32::DARK_RED,reason);}
                    if tool==Page::Services {self.services_ui(ui);}
                    else if tool==Page::Routes {ui.add_enabled_ui(block.is_none(),|ui|self.routes_ui(ui));}
                    else {self.service_manager(ui,tool);}
                    self.service_preview_ui(ui);
                }
                ui.separator();ui.label(&self.message);
                if !matches!(tool,Page::Services|Page::Forward|Page::Socks) {egui::CollapsingHeader::new("执行详情").show(ui,|ui|{ui.monospace(&self.output);});}
            });
        self.topology_tool=if open{Some(tool)}else{None};
    }
    fn services_ui(&mut self,ui:&mut egui::Ui){self.services_ui_filtered(ui,"");}
    fn services_ui_filtered(&mut self,ui:&mut egui::Ui,filter:&str) {
        if filter!="socks" {
            std::mem::swap(&mut self.services,&mut self.local_services);
            ui.push_id("mac-services",|ui|self.service_rows_ui(ui,filter,true));
            std::mem::swap(&mut self.services,&mut self.local_services);
            ui.separator();
        }
        ui.push_id("guest-services",|ui|self.service_rows_ui(ui,filter,false));
    }
    fn service_rows_ui(&mut self,ui:&mut egui::Ui,filter:&str,local:bool) {
        ui.strong(if local {"宿主机 Mac · 本地转发入口".into()}else{format!("{} · 虚拟机内转发与代理",self.vm)});
        ui.horizontal_wrapped(|ui|{
            if ui.add_enabled(self.rx.is_none(),egui::Button::new("刷新服务与日志")).clicked(){self.services.refresh=true;}
            ui.checkbox(&mut self.services.auto,"每 5 秒刷新");
            if let Some(t)=self.services.loaded {ui.label(format!("{} 秒前读取",t.elapsed().as_secs()));}
            if self.rx.is_some(){ui.spinner();}
        });
        if !self.services.error.is_empty(){ui.colored_label(Color32::DARK_RED,format!("读取失败，保留旧数据：{}",self.services.error));}
        if !self.services.result.is_empty(){
            egui::CollapsingHeader::new("操作结果").default_open(true).show(ui,|ui|{egui::ScrollArea::vertical().max_height(150.).show(ui,|ui|{ui.monospace(&self.services.result);});});
        }
        if self.services.data.is_null(){ui.label(if local{"等待从 Mac 读取本地转发规则，不自动启动转发。"}else{"等待从所选虚拟机读取服务，不自动启动任何服务。"});return;}
        ui.small(hostnet::field(&self.services.data,"note"));
        let rows=hostnet::array(&self.services.data["services"]).iter().filter(|r|filter.is_empty()||hostnet::field(r,"kind")==filter).cloned().collect::<Vec<_>>();
        let count=rows.iter().filter(|r|r["running"]==true).count();
        ui.label(format!("正在监听 {count} / 已知服务 {}（{}）",rows.len(),if local{"此 Mac 的全部本地规则"}else{"仅当前虚拟机"}));
        if rows.is_empty(){ui.label("未找到本工具管理的代理或已知转发。");}
        egui::ScrollArea::horizontal().id_salt("services-table-scroll").show(ui,|ui|{
            egui::Grid::new("services-table").striped(true).show(ui,|ui|{
                for h in ["状态 / 类型","访问入口 / 运行位置","实际目标","允许来源 IP / 网段","当前连接","操作"]{ui.strong(h);}ui.end_row();
                for row in &rows {
                    let id=service_id(row);
                    let running=row["running"]==true;
                    let endpoint=format!("{}:{}",hostnet::field(row,"ip"),row["port"]);
                    if ui.selectable_label(self.services.selected==id,format!("{} · {}",if running{"监听中"}else if hostnet::field(row,"status")=="unknown"{"无法确认"}else{"未监听"},hostnet::field(row,"kind"))).clicked(){self.services.selected=id.clone();}
                    ui.vertical(|ui|{ui.label(&endpoint);ui.small(if local{"运行于：宿主机 Mac".into()}else{format!("运行于：{}",self.vm)});});ui.vertical(|ui|{ui.set_min_width(170.);ui.label(if hostnet::field(row,"kind")=="socks"{"按请求转发"}else{hostnet::field(row,"target")});if let Some(e)=row["last_error"].as_str().filter(|s|!s.is_empty()){egui::CollapsingHeader::new("最近一次错误（历史记录）").id_salt(service_id(row)).show(ui,|ui|{ui.set_max_width(300.);ui.add(egui::Label::new(e).wrap());});}});
                    let allow=hostnet::field(row,"allow");
                    ui.add(egui::Label::new(if allow.is_empty(){"未提供 / 待确认"}else if allow=="0.0.0.0/0"{"全部 IPv4 来源"}else{allow}).wrap()).on_hover_text("按连接的源 IP 判断访问权限；不代表已验证网络连通。/32 表示单个来源 IP。");
                    ui.label(row["stats"]["active"].as_u64().map(|v|v.to_string()).unwrap_or("—".into()));
                    ui.vertical(|ui|{
                      ui.horizontal(|ui|{
                        if ui.button("日志").clicked(){self.services.selected=id;}
                        if hostnet::field(row,"kind")=="socks" && ui.button("复制地址").clicked(){ui.ctx().copy_text(endpoint.clone());}
                        if hostnet::field(row,"kind")=="forward" {
                            let url=format!("http://{endpoint}");
                            if ui.button("复制网址").clicked(){ui.ctx().copy_text(url.clone());}
                            if ui.add_enabled(running,egui::Button::new("打开入口")).clicked(){let _=Command::new("/usr/bin/open").arg(&url).spawn();}
                        }
                      });
                      ui.horizontal(|ui|{
                        if local && ui.add_enabled(self.rx.is_none(),egui::Button::new("检查链路")).clicked(){self.service_change(row,"check");}
                        if ui.add_enabled(running&&self.rx.is_none(),egui::Button::new("谁能访问")).on_hover_text("在拓扑中查看此运行服务允许哪些来源；已停止服务保留历史规则。").clicked(){
                            if local || self.vms.iter().any(|g|g.name==self.vm){
                                self.topology.selected=if local{format!("service:host:forward:{endpoint}")}else{let g=self.vms.iter().find(|g|g.name==self.vm).unwrap();format!("service:vm:{}:{}",g.id,service_id(row))};
                                self.topology.query.clear();self.topology.access_view=true;self.topology.reset();self.topology_services.refresh=true;
                                self.topology_tool=None;self.page=Page::Topology;self.launch(vec![],"host-read");
                            }
                        }
                      });
                      ui.horizontal(|ui|{
                        let managed=row["managed"]==true&&self.rx.is_none();
                        if ui.add_enabled(managed&&hostnet::field(row,"status")!="unknown",egui::Button::new(if running{"停止"}else{"开启"})).clicked(){self.service_change(row,if running{"stop"}else{"start"});}
                        if ui.add_enabled(managed,egui::Button::new("删除")).clicked(){self.service_change(row,"delete");}
                      });
                    });ui.end_row();
                }
            });
        });
        ui.small("允许来源是访问规则；当前连接为 0 表示没有活跃连接，并不表示服务故障。点击「谁能访问」查看来源主机与入口的关系。");
        if let Some(row)=rows.iter().find(|r|service_id(r)==self.services.selected) {
            ui.separator();ui.strong(format!("连接日志 · {}:{}",hostnet::field(row,"ip"),row["port"]));
            if hostnet::field(row,"telemetry")=="connections" {
                ui.label(format!("本次进程累计：接入 {}  拒绝 {}  失败 {}  发送 {} B  接收 {} B",row["stats"]["accepted"],row["stats"]["rejected"],row["stats"]["failed"],row["stats"]["tx"],row["stats"]["rx"]));
            } else {ui.small("此服务没有完整历史遥测；旧版本进程需手动停止并重建后才能记录日志。");}
            if let Some(e)=row["log_error"].as_str().filter(|e|!e.is_empty()){ui.colored_label(Color32::DARK_RED,format!("日志落盘失败：{e}"));}
            ui.horizontal(|ui|{
                ui.add(egui::TextEdit::singleline(&mut self.services.query).hint_text("搜索来源 / 目标 / 错误"));
                if ui.button("复制连接日志").clicked(){ui.ctx().copy_text(serde_json::to_string_pretty(&row["events"]).unwrap_or_default());}
            });
            ui.small("时间为 UTC（Mac 使用 Unix 秒）；发送 / 接收相对于客户端。仅连接元数据，不保存报文、URL、密码。最近 200 条，磁盘日志轮转保留约 2 MiB / 服务。");
            egui::ScrollArea::both().id_salt("service-events").max_height(260.).show(ui,|ui|{
                for e in hostnet::array(&row["events"]).iter().rev() {
                    let line=format!("{}  {}  {} → {}  ↑{} B ↓{} B  {} ms  {}",hostnet::field(e,"time"),hostnet::field(e,"event"),hostnet::field(e,"peer"),hostnet::field(e,"target"),e["tx"].as_u64().map(|v|v.to_string()).unwrap_or("—".into()),e["rx"].as_u64().map(|v|v.to_string()).unwrap_or("—".into()),e["duration_ms"].as_u64().map(|v|v.to_string()).unwrap_or("—".into()),hostnet::field(e,"error"));
                    if line.to_lowercase().contains(&self.services.query.to_lowercase()){ui.monospace(line);}
                }
            });
        } else {ui.label("选择一条服务查看连接日志。");}
    }
}
