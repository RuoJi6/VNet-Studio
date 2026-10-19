#[derive(Default)]
struct TopologyServices {
    rows: std::collections::BTreeMap<String,(serde_json::Value,std::time::Instant,String)>,
    rx: Option<mpsc::Receiver<(String,Result<serde_json::Value,String>)>>,
    refresh: bool,
    loaded: Option<std::time::Instant>,
}
impl App {
    fn topology_services_tick(&mut self,ctx:&egui::Context) {
        if let Some(rx)=&self.topology_services.rx {
            loop {match rx.try_recv() {
                Ok((id,result))=>{
                    let (data,error)=match result {Ok(v)=>(v,String::new()),Err(e)=>(serde_json::Value::Null,e)};
                    self.topology_services.rows.insert(id,(data,std::time::Instant::now(),error));
                },
                Err(mpsc::TryRecvError::Disconnected)=>{self.topology_services.rx=None;break},
                Err(mpsc::TryRecvError::Empty)=>break,
            }}
        }
        if self.topology_services.rx.is_some(){ctx.request_repaint_after(Duration::from_millis(200));return;}
        if self.page!=Page::Topology || self.host.is_null(){return;}
        let due=self.topology.auto && self.topology_services.loaded.map(|t|t.elapsed()>=Duration::from_secs(10)).unwrap_or(true);
        if !self.topology_services.refresh && self.topology_services.loaded.is_some() && !due {return;}
        self.topology_services.refresh=false;
        self.topology_services.loaded=Some(std::time::Instant::now());
        let guests=self.vms.iter().filter(|g|g.state=="running").cloned().collect::<Vec<_>>();
        let (tx,rx)=mpsc::channel();self.topology_services.rx=Some(rx);
        let local_tx=tx.clone();
        let cli=cli_path().to_string_lossy().to_string();
        std::thread::spawn(move||{
            let result=hostnet::command(&cli,&["host-forward".into(),"list".into()]).and_then(|s|serde_json::from_str(&s).map_err(|e|e.to_string()));
            let _=local_tx.send(("host".into(),result));
        });
        for guest in guests {
            let tx=tx.clone();let cli=cli_path().to_string_lossy().to_string();let prl=prl_path();
            std::thread::spawn(move||{
                let result=guest.check().and_then(|_|{
                    hostnet::command(&cli,&["--vm".into(),guest.id.clone(),"services".into(),"list".into(),"--prlctl".into(),prl])
                }).and_then(|text|serde_json::from_str::<serde_json::Value>(&text).map_err(|e|e.to_string())).and_then(|v|if v["services"].is_array(){Ok(v)}else{Err("无法解析服务列表".into())});
                let _=tx.send((guest.id,result));
            });
        }
    }
    fn topology_open_service(&mut self,vm:&str,row:&serde_json::Value){
        if row["location"]=="host" {
            self.topology_tool=Some(Page::Services);self.local_services.selected=service_id(row);self.local_services.refresh=true;
            if let Some((data,at,error))=self.topology_services.rows.get("host"){self.local_services.data=data.clone();self.local_services.loaded=Some(*at);self.local_services.error=error.clone();}
            return;
        }
        self.open_topology_tool(vm,Page::Services);
        self.services.selected=service_id(row);
        if let Some(g)=self.vms.iter().find(|g|g.name==vm){
            if let Some((data,at,error))=self.topology_services.rows.get(&g.id){
                self.services.data=data.clone();self.services.loaded=Some(*at);self.services.error=error.clone();
            }
        }
    }
}
impl App {
    fn topology_ui(&mut self,ui:&mut egui::Ui){
        ui.add_space(10.);ui.heading("网络拓扑");
        ui.horizontal_wrapped(|ui|{
            ui.label("查看方式");
            if ui.selectable_value(&mut self.topology.access_view,true,"谁能访问").changed(){self.topology.reset();}
            if ui.selectable_value(&mut self.topology.access_view,false,"网络连接").changed(){self.topology.reset();}
        });
        ui.label(if self.topology.access_view {"访问来源 → Mac / 虚拟机上的服务入口 → 目标服务"}else{"Mac → 网卡 / 虚拟网络 → 虚拟机 → 转发 / 代理 → 目标"});
        ui.horizontal_wrapped(|ui|{
            if ui.add_enabled(self.rx.is_none(),egui::Button::new("刷新拓扑")).clicked(){self.topology.attempt=Some(std::time::Instant::now());self.launch(vec![],"host-read");}
            ui.checkbox(&mut self.topology.auto,"每 10 秒刷新");
            if let Some(at)=self.topology.loaded{ui.label(format!("{} 秒前读取",at.elapsed().as_secs()));}else{ui.label("等待读取网络状态");}
            if self.rx.is_some(){ui.spinner();}
        });
        if !self.topology.error.is_empty(){ui.colored_label(Color32::DARK_RED,format!("刷新失败，图中保留旧数据：{}",self.topology.error));}
        if !hostnet::array(&self.host["errors"]).is_empty(){ui.colored_label(Color32::from_rgb(165,103,24),"部分信息未能读取；缺失信息不代表设备不存在。");egui::CollapsingHeader::new("读取错误").show(ui,|ui|{for e in hostnet::array(&self.host["errors"]){ui.label(e.as_str().unwrap_or("未知错误"));}});}
        ui.horizontal_wrapped(|ui|{
            if ui.add(egui::TextEdit::singleline(&mut self.topology.query).hint_text("筛选虚拟机名称 / IP / 系统").desired_width(220.)).changed(){self.topology.reset();}
            if ui.checkbox(&mut self.topology.running_only,"仅运行中").changed(){self.topology.reset();}
            if ui.checkbox(&mut self.topology.show_inactive,"显示未连接设备").changed(){self.topology.reset();}
            if ui.button("−").clicked(){self.topology.zoom=(self.topology.zoom/1.15).max(0.35);}
            ui.label(format!("{:.0}%",self.topology.zoom*100.));
            if ui.button("＋").clicked(){self.topology.zoom=(self.topology.zoom*1.15).min(1.6);}
            if ui.button("重置布局").clicked(){self.topology.reset();}
        });
        ui.small(if self.topology.access_view {"青虚线：来源 IP 符合规则   灰虚线：未允许 / 未确认 / 配置关系   紫：转发与代理"}else{"蓝：共享 NAT   青：桥接 / 物理接口   紫：仅主机 / USB / 服务   灰虚线：未运行、断开或待确认"});
        ui.small("点击连线高亮，点击空白取消；拖动空白平移，拖动卡片调整位置；右键操作或双击查看服务。连线表示配置和归属，不代表服务或互联网已连通。");
        if self.host.is_null(){ui.add_space(25.);ui.label("正在读取 PD 网络与宿主机接口；此页面不会修改网络。");return;}
        let mut graph=topology::build(&self.host,&self.topology.query,self.topology.running_only,self.topology.show_inactive);
        let snapshots=self.topology_services.rows.iter().map(|(id,(data,at,error))|(id.clone(),serde_json::json!({"data":data,"stale":at.elapsed()>Duration::from_secs(30)||!error.is_empty()}))).collect();
        topology::add_services(&mut graph,&snapshots);
        if self.topology.access_view {
            ui.small("访问来源 → 服务入口 → 目标。虚线为 ACL 判断，不保证连通；点击服务聚焦，点击空白查看全部。");
            let access=topology::access_graph(&graph,&self.host,&self.topology.selected);
            if access.nodes.is_empty(){
                ui.colored_label(Color32::from_rgb(165,103,24),if self.topology_services.rx.is_some(){"正在读取服务，先显示网络连接。"}else{"未发现可显示的运行服务，先显示网络连接。已停止规则可在端口转发 / 代理列表中开启。"});
            }else{graph=access;}

        }
        ui.horizontal_wrapped(|ui|{
            if ui.add_enabled(self.topology_services.rx.is_none(),egui::Button::new("刷新服务状态")).clicked(){self.topology_services.refresh=true;}
            if self.topology_services.rx.is_some(){ui.spinner();ui.label("正在读取运行中虚拟机的服务…");}
            if let Some(at)=self.topology_services.loaded{ui.small(format!("服务采集发起于 {} 秒前；超过 30 秒以灰色旧快照显示",at.elapsed().as_secs()));}
        });
        if let Some((_,_,error))=self.topology_services.rows.get("host"){if !error.is_empty(){ui.colored_label(Color32::DARK_RED,format!("Mac 转发读取失败：{error}"));}}
        for g in self.vms.iter().filter(|g|g.state=="running"){
            match self.topology_services.rows.get(&g.id){
                Some((data,_,error)) if error.is_empty()=>{for e in hostnet::array(&data["errors"]){ui.colored_label(Color32::DARK_RED,format!("{}：{e}",g.name));}},
                Some((_,_,error))=>{egui::CollapsingHeader::new(format!("{}：服务读取失败（不能判断是否开启）",g.name)).show(ui,|ui|{ui.label(error);});},
                None=>{ui.small(format!("{}：服务状态待读取",g.name));},
            }
        }
        ui.small("只显示检测到的运行服务；停止后刷新会移除节点，历史记录仍在「运行与日志」。SOCKS5 目标按当前连接快照显示。");
        if !graph.nodes.iter().any(|n|n.vm.is_some()){ui.label(if self.topology.access_view {"当前没有可显示的运行服务；未能读取的虚拟机请查看上方提示。"}else{"当前筛选下没有虚拟机。"});}
        if let Some(n)=graph.nodes.iter().find(|n|n.id==self.topology.selected) {
            if let Some(vm)=&n.vm {ui.add_enabled_ui(self.rx.is_none(),|ui|{ui.horizontal_wrapped(|ui|{
                ui.strong(&n.title);
                if let Some(row)=&n.service {
                    if ui.button("查看此服务日志").clicked(){self.topology_open_service(vm,row);}
                    if row["managed"]==true && ui.button("停止此服务").clicked(){
                        self.topology_open_service(vm,row);self.service_change(row,"stop");
                    }
                    if row["managed"]==true && ui.button("删除此规则").clicked(){
                        self.topology_open_service(vm,row);self.service_change(row,"delete");
                    }
                }
                for (p,label) in [(Page::Host,"网络设置"),(Page::Routes,"默认出口 / 路由"),(Page::Forward,"添加转发"),(Page::Socks,"配置代理"),(Page::Services,"运行与日志")] {
                    if ui.button(label).clicked(){if p==Page::Services {if let Some(row)=&n.service {self.topology_open_service(vm,row);}else{self.open_topology_tool(vm,p);}}else{self.open_topology_tool(vm,p);}}
                }
            });});}
        } else {ui.small("选择虚拟机后可直接设置网络、转发与代理；右键卡片也可操作。");}
        self.topology.canvas(ui,&graph);
        if let Some((vm,action))=self.topology.action.take(){if self.rx.is_none(){let page=match action.as_str(){"network"=>Page::Host,"routes"=>Page::Routes,"forward"=>Page::Forward,"socks"=>Page::Socks,_=>Page::Services};if page==Page::Services {if let Some(row)=graph.nodes.iter().find(|n|n.id==self.topology.selected).and_then(|n|n.service.as_ref()){self.topology_open_service(&vm,row);}else{self.open_topology_tool(&vm,page);}}else{self.open_topology_tool(&vm,page);}}}

        ui.horizontal(|ui|{
            ui.label(format!("{} 个节点 · {} 条关系",graph.nodes.len(),graph.edges.len()));
            egui::ComboBox::from_id_salt("topology-node-list").selected_text("选择节点查看详情").width(230.).show_ui(ui,|ui|{
                for n in &graph.nodes{if ui.selectable_value(&mut self.topology.selected,n.id.clone(),format!("{} · {}",n.kind,n.title)).changed(){self.topology.selected_edge=None;}}
            });
        });
        if let Some(key)=self.topology.selected_edge.clone(){
            let name=|id:&str|graph.nodes.iter().find(|n|n.id==id).map(|n|n.title.as_str()).unwrap_or("未知");
            ui.horizontal_wrapped(|ui|{
                ui.colored_label(Color32::from_rgb(222,104,20),format!("选中路线：{} → {} · {}",name(&key.0),name(&key.1),key.2));
                if ui.button("取消高亮").clicked(){self.topology.selected_edge=None;}
            });
            ui.small("高亮仅用于查看路径，不代表访问已允许或服务已连通；原有虚线含义保持不变。");
        }
        if let Some(n)=graph.nodes.iter().find(|n|n.id==self.topology.selected){
            egui::Frame::group(ui.style()).show(ui,|ui|{
                ui.horizontal_wrapped(|ui|{ui.strong(&n.title);ui.label(&n.status);if let Some(vm)=&n.vm{if ui.button("管理此虚拟机网络").clicked(){self.open_topology_tool(vm,Page::Host);}}});
                for line in &n.lines{ui.label(line);}
                if let Some(row)=&n.service {
                    ui.strong(format!("允许的客户端：{}",if hostnet::field(row,"allow").is_empty(){"未提供，不能判断"}else if hostnet::field(row,"allow")=="0.0.0.0/0"{"全部 IPv4 来源"}else{hostnet::field(row,"allow")}));
                    egui::Grid::new("access-client-table").striped(true).show(ui,|ui|{
                        ui.strong("访问来源");ui.strong("已知 IPv4");ui.strong("访问规则判断");ui.end_row();
                        for (_,name,ips) in topology::access_clients(&self.host){let (status,matching)=topology::access_status(&ips,hostnet::field(row,"allow"));ui.label(name);ui.label(ips.join(", "));ui.label(if matching.is_empty(){status}else{format!("{status}：{}",matching.join(", "))});ui.end_row();}
                    });
                    ui.small("规则按来源 IP 控制，不是按虚拟机身份；IP 变化后需要更新。路由、NAT 与其他防火墙规则仍会影响实际访问。");
                    if let Some(vm)=&n.vm{if row["managed"]==true && ui.add_enabled(self.rx.is_none(),egui::Button::new(if row["location"]=="host"{"按此配置填写 Mac 本地转发…"}else{"按此配置调整访问范围…"})).clicked(){
                        self.open_topology_tool(vm,if hostnet::field(row,"kind")=="socks"{Page::Socks}else{Page::Forward});
                        self.local_forward=row["location"]=="host";
                        if self.local_forward {self.local_ip=hostnet::field(row,"ip").into();self.local_port=row["port"].to_string();self.local_target=hostnet::field(row,"target").into();self.local_batch_mode=false;}
                        else {self.listen_ip=hostnet::field(row,"ip").into();self.port=row["port"].to_string();self.socks_port=self.port.clone();self.target=hostnet::field(row,"target").into();self.allow=hostnet::field(row,"allow").into();self.services.access_mode.clear();}
                    }}
                }
                egui::CollapsingHeader::new("连接关系与原始状态").show(ui,|ui|{
                    for e in graph.edges.iter().filter(|e|e.from==n.id||e.to==n.id){let name=|id:&str|graph.nodes.iter().find(|n|n.id==id).map(|n|n.title.as_str()).unwrap_or("未知");ui.label(format!("{} → {}：{}",name(&e.from),name(&e.to),e.label));}
                    egui::ScrollArea::vertical().max_height(150.).show(ui,|ui|{ui.monospace(&n.detail);});
                });
            });
        }
        egui::CollapsingHeader::new("文字版拓扑 / 连接清单").show(ui,|ui|{
            egui::ScrollArea::vertical().max_height(180.).show(ui,|ui|{for e in &graph.edges{let name=|id:&str|graph.nodes.iter().find(|n|n.id==id).map(|n|n.title.as_str()).unwrap_or("未知");ui.label(format!("{} → {}：{}",name(&e.from),name(&e.to),e.label));}});
        });
    }
}
