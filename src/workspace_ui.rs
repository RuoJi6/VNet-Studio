const INK: Color32 = Color32::from_rgb(28, 30, 34);
const MUTED: Color32 = Color32::from_rgb(124, 129, 138);
const LINE: Color32 = Color32::from_rgb(229, 231, 235);
const GREEN: Color32 = Color32::from_rgb(30, 166, 91);
#[derive(Default)]
struct ForwardWorkspace {
    search: String,
    scope: u8,
    selected: String,
    detail_tab: u8,
}
fn page_title(page: Page) -> &'static str {
    match page {
        Page::Overview => "网络概览",
        Page::Host => "本机与虚拟机网络",
        Page::Topology => "网络拓扑",
        Page::Forward => "端口转发",
        Page::Socks => "SOCKS5 代理",
        Page::Vpn => "VPN 连接",
        Page::Containers => "容器状态",
        Page::Services => "运行与日志",
        Page::Routes => "默认出口与路由",
    }
}
fn primary_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).color(Color32::WHITE))
        .fill(INK)
        .stroke(egui::Stroke::NONE)
}
fn input_field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
    ui.label(RichText::new(label).color(INK));
    ui.add_sized(
        [ui.available_width(), 34.],
        egui::TextEdit::singleline(value)
            .margin(egui::vec2(10., 8.))
            .hint_text(hint),
    );
}
impl App {
    fn navigate(&mut self, page: Page) {
        if self.page != page {
            self.output.clear();
            self.message.clear();
        }
        self.page = page;
        self.topology_tool = None;
        self.pending = None;
        self.pending_host = None;
        if matches!(page, Page::Forward | Page::Socks | Page::Services) {
            self.services.refresh = true;
            self.local_services.refresh = true;
        }
        if matches!(page, Page::Host | Page::Topology) && self.rx.is_none() {
            self.topology.attempt = Some(std::time::Instant::now());
            self.launch(vec![], "host-read");
        }
    }
    fn desktop_shell(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("desktop-toolbar")
            .exact_height(54.)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(250, 250, 251))
                    .inner_margin(egui::Margin::symmetric(18, 9))
                    .stroke(egui::Stroke::new(1., LINE)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("VNet Studio").size(17.).strong().color(INK));
                    ui.add_space(20.);
                    ui.separator();
                    ui.add_space(12.);
                    if ui.button("＋  新建转发").clicked() {
                        self.forward_workspace.selected.clear();
                        self.navigate(Page::Forward);
                    }
                    if ui.button("◇  网络拓扑").clicked() {
                        self.navigate(Page::Topology);
                    }
                    if ui.button("≡  运行与日志").clicked() {
                        self.navigate(Page::Services);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(self.rx.is_none(), egui::Button::new("↻  刷新虚拟机"))
                            .clicked()
                        {
                            self.launch(vec![], "vms");
                        }
                        let running = self.vms.iter().filter(|g| g.state == "running").count();
                        ui.label(
                            RichText::new(format!("{running} 台运行中"))
                                .size(12.)
                                .color(MUTED),
                        );
                    });
                });
            });
        egui::TopBottomPanel::bottom("desktop-status")
            .exact_height(28.)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(249, 250, 251))
                    .inner_margin(egui::Margin::symmetric(16, 5))
                    .stroke(egui::Stroke::new(1., LINE)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.rx.is_some() {
                        ui.spinner();
                    }
                    ui.label(
                        RichText::new(if self.message.is_empty() {
                            "就绪"
                        } else {
                            &self.message
                        })
                        .size(12.)
                        .color(if self.good {
                            MUTED
                        } else {
                            Color32::from_rgb(190, 61, 61)
                        }),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                                .size(11.)
                                .color(MUTED),
                        );
                        ui.label(
                            RichText::new("本地管理 · Parallels Desktop")
                                .size(11.)
                                .color(MUTED),
                        );
                    });
                });
            });
        egui::SidePanel::left("desktop-navigation")
            .exact_width(194.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(248, 249, 250))
                    .inner_margin(egui::Margin::symmetric(12, 18))
                    .stroke(egui::Stroke::new(1., LINE)),
            )
            .show(ctx, |ui| {
                ui.label(RichText::new("工作空间").size(11.).color(MUTED));
                ui.add_space(10.);
                for (page, icon) in [
                    (Page::Overview, "▦"),
                    (Page::Topology, "◇"),
                    (Page::Host, "⌘"),
                    (Page::Routes, "⇢"),
                    (Page::Forward, "⇄"),
                    (Page::Socks, "◎"),
                    (Page::Vpn, "◈"),
                    (Page::Containers, "▣"),
                    (Page::Services, "≡"),
                ] {
                    if page == Page::Forward || page == Page::Containers {
                        ui.add_space(12.);
                        ui.separator();
                        ui.add_space(6.);
                    }
                    let active = self.page == page;
                    let text = RichText::new(format!("{icon}   {}", page_title(page)))
                        .size(13.5)
                        .color(if active {
                            INK
                        } else {
                            Color32::from_rgb(89, 94, 103)
                        });
                    let button = egui::Button::new(if active { text.strong() } else { text })
                        .fill(if active {
                            Color32::from_rgb(231, 233, 237)
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(egui::Stroke::NONE);
                    if ui.add_sized([ui.available_width(), 36.], button).clicked() {
                        self.navigate(page);
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new("Windows / Linux / macOS")
                            .size(11.)
                            .color(MUTED),
                    );
                    ui.label(RichText::new("虚拟机网络工作台").size(12.).color(MUTED));
                });
            });
    }
    fn compact_vm_picker(&mut self, ui: &mut egui::Ui, id: &str) {
        let selected = self
            .vms
            .iter()
            .find(|g| g.name == self.vm)
            .map(|g| format!("{} · {}", g.name, state_label(&g.state)))
            .unwrap_or_else(|| {
                if self.vm.is_empty() {
                    "选择虚拟机".into()
                } else {
                    self.vm.clone()
                }
            });
        egui::ComboBox::from_id_salt(id)
            .selected_text(selected)
            .width(ui.available_width().max(120.))
            .height(240.)
            .show_ui(ui, |ui| {
                let mut guests = self.vms.iter().collect::<Vec<_>>();
                guests.sort_by_key(|g| g.state != "running");
                for g in guests {
                    ui.selectable_value(&mut self.vm, g.name.clone(), vm_choice_text(g));
                }
            });
        self.sync_selection();
    }
    fn workspace_rows(&self, page: Page) -> Vec<serde_json::Value> {
        let kind = if page == Page::Socks {
            "socks"
        } else {
            "forward"
        };
        let mut rows = vec![];
        if page != Page::Socks {
            rows.extend(
                hostnet::array(&self.local_services.data["services"])
                    .iter()
                    .cloned(),
            );
        }
        rows.extend(
            hostnet::array(&self.services.data["services"])
                .iter()
                .filter(|r| hostnet::field(r, "kind") == kind)
                .cloned(),
        );
        rows
    }
    fn forward_workspace_ui(&mut self, ui: &mut egui::Ui, page: Page) {
        egui::Frame::new()
            .inner_margin(egui::Margin::same(22))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new(page_title(page)).size(21.).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("TCP").size(12.).color(MUTED));
                    });
                });
                ui.label(
                    RichText::new(if page == Page::Forward {
                        "管理访问入口与目标服务，清楚地连接 Mac 和虚拟机。"
                    } else {
                        "管理所选虚拟机中的 SOCKS5 代理与连接记录。"
                    })
                    .size(13.)
                    .color(MUTED),
                );
                ui.add_space(18.);
                let rows = self.workspace_rows(page);
                let selected = rows
                    .iter()
                    .find(|r| service_id(r) == self.forward_workspace.selected)
                    .cloned();
                if selected.is_none() {
                    self.forward_workspace.selected.clear();
                }
                let height = ui.available_height().max(420.);
                let total = ui.available_width();
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(egui::Stroke::new(1., LINE))
                    .corner_radius(8)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 0.;
                        ui.horizontal_top(|ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(218., height - 2.),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    egui::Frame::new()
                                        .inner_margin(egui::Margin::same(14))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.strong("转发规则");
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        if ui
                                                            .small_button("＋")
                                                            .on_hover_text("新建规则")
                                                            .clicked()
                                                        {
                                                            self.forward_workspace.selected.clear();
                                                        }
                                                    },
                                                );
                                            });
                                            ui.add_space(12.);
                                            ui.add_sized(
                                                [190., 32.],
                                                egui::TextEdit::singleline(
                                                    &mut self.forward_workspace.search,
                                                )
                                                .hint_text("搜索入口、目标…"),
                                            );
                                            ui.add_space(10.);
                                            ui.horizontal(|ui| {
                                                for (v, label) in
                                                    [(0, "全部"), (1, "Mac"), (2, "虚拟机")]
                                                {
                                                    ui.selectable_value(
                                                        &mut self.forward_workspace.scope,
                                                        v,
                                                        label,
                                                    );
                                                }
                                            });
                                        });
                                    ui.separator();
                                    egui::ScrollArea::vertical().id_salt("rule-list").show(
                                        ui,
                                        |ui| {
                                            let mut count = 0;
                                            for row in &rows {
                                                let local = row["location"] == "host";
                                                let id = service_id(row);
                                                let endpoint = format!(
                                                    "{}:{}",
                                                    hostnet::field(row, "ip"),
                                                    row["port"]
                                                );
                                                if (self.forward_workspace.scope == 1 && !local)
                                                    || (self.forward_workspace.scope == 2 && local)
                                                {
                                                    continue;
                                                }
                                                if !format!(
                                                    "{endpoint} {}",
                                                    hostnet::field(row, "target")
                                                )
                                                .to_lowercase()
                                                .contains(
                                                    &self.forward_workspace.search.to_lowercase(),
                                                ) {
                                                    continue;
                                                }
                                                count += 1;
                                                let active = self.forward_workspace.selected == id;
                                                egui::Frame::new()
                                                    .fill(if active {
                                                        Color32::from_rgb(239, 243, 249)
                                                    } else {
                                                        Color32::WHITE
                                                    })
                                                    .inner_margin(egui::Margin::same(14))
                                                    .show(ui, |ui| {
                                                        ui.set_width(190.);
                                                        let response = ui.add(
                                                            egui::Button::new(
                                                                RichText::new(&endpoint).strong(),
                                                            )
                                                            .frame(false)
                                                            .min_size(egui::vec2(190., 24.)),
                                                        );
                                                        if response.clicked() {
                                                            self.forward_workspace.selected = id;
                                                            self.forward_workspace.detail_tab = 0;
                                                        }
                                                        ui.horizontal(|ui| {
                                                            ui.label(
                                                                RichText::new("●").size(9.).color(
                                                                    if row["running"] == true {
                                                                        GREEN
                                                                    } else {
                                                                        MUTED
                                                                    },
                                                                ),
                                                            );
                                                            ui.small(if local {
                                                                "本地 Mac"
                                                            } else {
                                                                "虚拟机"
                                                            });
                                                            ui.small(if row["running"] == true {
                                                                "运行中"
                                                            } else {
                                                                "已停止"
                                                            });
                                                        });
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "→ {}",
                                                                if page == Page::Socks {
                                                                    "按请求转发"
                                                                } else {
                                                                    hostnet::field(row, "target")
                                                                }
                                                            ))
                                                            .size(11.)
                                                            .color(MUTED),
                                                        );
                                                    });
                                                ui.separator();
                                            }
                                            if count == 0 {
                                                ui.add_space(100.);
                                                ui.vertical_centered(|ui| {
                                                    ui.label(
                                                        RichText::new("⇄")
                                                            .size(30.)
                                                            .color(Color32::from_gray(204)),
                                                    );
                                                    ui.add_space(8.);
                                                    ui.label(
                                                        RichText::new(if rows.is_empty() {
                                                            "暂无转发规则"
                                                        } else {
                                                            "没有匹配的规则"
                                                        })
                                                        .size(13.)
                                                        .color(MUTED),
                                                    );
                                                    ui.small("点击 ＋ 创建一条规则");
                                                });
                                            }
                                        },
                                    );
                                    ui.with_layout(
                                        egui::Layout::bottom_up(egui::Align::LEFT),
                                        |ui| {
                                            egui::Frame::new()
                                                .inner_margin(egui::Margin::same(14))
                                                .show(ui, |ui| {
                                                    if ui
                                                        .add_enabled(
                                                            self.rx.is_none(),
                                                            egui::Button::new("↻  刷新规则")
                                                                .min_size(egui::vec2(190., 30.)),
                                                        )
                                                        .clicked()
                                                    {
                                                        self.local_services.refresh = true;
                                                        self.services.refresh = true;
                                                    }
                                                    ui.small(format!(
                                                        "{} 条规则 · {} 条运行中",
                                                        rows.len(),
                                                        rows.iter()
                                                            .filter(|r| r["running"] == true)
                                                            .count()
                                                    ));
                                                });
                                        },
                                    );
                                },
                            );
                            let (line, _) = ui.allocate_exact_size(
                                egui::vec2(1., height - 2.),
                                egui::Sense::hover(),
                            );
                            ui.painter().rect_filled(line, 0., LINE);
                            ui.allocate_ui_with_layout(
                                egui::vec2((total - 222.).max(300.), height - 2.),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(10., 10.);
                                    egui::TopBottomPanel::bottom("rule-editor-actions")
                                        .frame(
                                            egui::Frame::new()
                                                .fill(Color32::from_rgb(251, 251, 252))
                                                .inner_margin(egui::Margin::symmetric(22, 14))
                                                .stroke(egui::Stroke::new(1., LINE)),
                                        )
                                        .show_inside(ui, |ui| {
                                            if let Some(row) = &selected {
                                                self.workspace_rule_actions(ui, row);
                                            } else {
                                                self.rule_submit_ui(ui, page);
                                            }
                                        });
                                    egui::CentralPanel::default()
                                        .frame(
                                            egui::Frame::new().inner_margin(egui::Margin::same(24)),
                                        )
                                        .show_inside(ui, |ui| {
                                            egui::ScrollArea::vertical()
                                                .id_salt("rule-editor-scroll")
                                                .show(ui, |ui| {
                                                    if let Some(row) = &selected {
                                                        self.workspace_rule_detail(ui, row);
                                                    } else {
                                                        self.rule_form_fields(ui, page);
                                                    }
                                                    self.workspace_result(ui);
                                                });
                                        });
                                },
                            );
                        });
                    });
            });
    }
    fn rule_form_fields(&mut self, ui: &mut egui::Ui, page: Page) {
        let local = page == Page::Forward && self.local_forward;
        ui.horizontal(|ui| {
            ui.strong("新建规则");
            ui.label(
                RichText::new("填写入口和实际服务地址")
                    .size(12.)
                    .color(MUTED),
            );
        });
        ui.add_space(22.);
        let width = ui.available_width();
        let col = (width - 68.) / 2.;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(col, 252.),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(col);
                    ui.colored_label(BLUE, RichText::new("访问入口").size(16.).strong());
                    ui.add_space(10.);
                    ui.label("转发运行位置");
                    egui::ComboBox::from_id_salt("rule-location")
                        .selected_text(if local {
                            "本地 Mac"
                        } else {
                            "所选虚拟机"
                        })
                        .width(col)
                        .show_ui(ui, |ui| {
                            if page == Page::Forward {
                                ui.selectable_value(&mut self.local_forward, true, "本地 Mac");
                            }
                            ui.selectable_value(&mut self.local_forward, false, "所选虚拟机");
                        });
                    if local {
                        input_field(ui, "Mac 监听 IP", &mut self.local_ip, "127.0.0.1");
                        if !self.local_batch_mode {
                            input_field(
                                ui,
                                "Mac 本地入口端口",
                                &mut self.local_port,
                                "填写未占用的端口",
                            );
                        }
                    } else {
                        ui.label("运行服务的虚拟机");
                        self.compact_vm_picker(ui, "source-vm");
                        self.ip_picker(ui);
                        let batch = self.services.batch_mode;
                        if !batch {
                            if page == Page::Socks {
                                input_field(
                                    ui,
                                    "SOCKS5 入口端口",
                                    &mut self.socks_port,
                                    "填写端口",
                                );
                            } else {
                                input_field(
                                    ui,
                                    "虚拟机入口端口",
                                    &mut self.port,
                                    "填写未占用的端口",
                                );
                            }
                        }
                    }
                    ui.label(
                        RichText::new(if local {
                            "浏览器访问此 Mac 的入口地址。"
                        } else {
                            "浏览器访问所选虚拟机的入口地址。"
                        })
                        .size(12.)
                        .color(MUTED),
                    );
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(48., 252.),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.add_space(62.);
                    ui.label(RichText::new("→").size(23.).color(MUTED));
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(col, 252.),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(col);
                    ui.colored_label(GREEN, RichText::new("实际目标").size(16.).strong());
                    ui.add_space(10.);
                    if page == Page::Socks {
                        ui.label("由 SOCKS5 客户端指定");
                        ui.label(
                            RichText::new("连接时按请求选择目标。支持 TCP CONNECT，不含 UDP。")
                                .color(MUTED),
                        );
                    } else {
                        ui.label(if local {
                            "关联虚拟机（可选）"
                        } else {
                            "当前虚拟机"
                        });
                        self.compact_vm_picker(ui, "target-vm");
                        let batch = if local {
                            self.local_batch_mode
                        } else {
                            self.services.batch_mode
                        };
                        if !batch {
                            if local {
                                input_field(
                                    ui,
                                    "实际服务 IPv4:端口",
                                    &mut self.local_target,
                                    "填写目标 IP:服务端口",
                                );
                            } else {
                                input_field(
                                    ui,
                                    "实际服务 IPv4:端口",
                                    &mut self.target,
                                    "填写目标 IP:服务端口",
                                );
                            }
                        }
                        let ips = self
                            .vms
                            .iter()
                            .find(|g| g.name == self.vm)
                            .map(|g| g.ips.join(", "))
                            .unwrap_or_default();
                        if !ips.is_empty() {
                            ui.label(
                                RichText::new(format!("所选虚拟机 IP：{ips}"))
                                    .size(12.)
                                    .color(MUTED),
                            );
                        }
                    }
                    ui.add_space(8.);
                    ui.label(
                        RichText::new(if local {
                            "由本地 Mac 连接目标。目标 127.0.0.1 指 Mac 自己。"
                        } else {
                            "由所选虚拟机连接目标。目标 127.0.0.1 指这台虚拟机自己。"
                        })
                        .size(12.)
                        .color(MUTED),
                    );
                },
            );
        });
        ui.add_space(18.);
        ui.separator();
        ui.add_space(14.);
        ui.horizontal(|ui| {
            ui.strong("录入方式");
            let value = if local {
                &mut self.local_batch_mode
            } else {
                &mut self.services.batch_mode
            };
            ui.selectable_value(value, false, "单条规则");
            ui.selectable_value(value, true, "批量录入");
        });
        let batch = if local {
            self.local_batch_mode
        } else {
            self.services.batch_mode
        };
        if batch {
            ui.small(if local {
                "每行：入口端口 目标IPv4:端口"
            } else if page == Page::Socks {
                "每行：代理端口 [允许来源网段]"
            } else {
                "每行：入口端口 目标IPv4:端口 [允许来源网段]"
            });
            let text = if local {
                &mut self.local_batch_text
            } else {
                &mut self.services.batch_text
            };
            ui.add_sized(
                [ui.available_width(), 100.],
                egui::TextEdit::multiline(text).hint_text("填写需要创建的规则，每行一条"),
            );
        }
        ui.add_space(12.);
        if local {
            ui.horizontal(|ui| {
                ui.strong("访问范围");
                ui.label("仅此 Mac · 回环地址 127.0.0.0/8");
            });
        } else {
            egui::CollapsingHeader::new(format!(
                "访问范围 · {}",
                if self.allow.is_empty() {
                    "请选择来源"
                } else {
                    &self.allow
                }
            ))
            .show(ui, |ui| self.access_picker(ui));
            if page == Page::Socks
                && !self
                    .vms
                    .iter()
                    .find(|g| g.name == self.vm)
                    .map(|g| g.unix())
                    .unwrap_or(true)
            {
                input_field(
                    ui,
                    "虚拟机内 GOST 路径",
                    &mut self.gost,
                    "gost.exe 或完整路径",
                );
            }
        }
        ui.add_space(18.);
        let ip = if local {
            &self.local_ip
        } else {
            &self.listen_ip
        };
        let port = if local {
            &self.local_port
        } else if page == Page::Socks {
            &self.socks_port
        } else {
            &self.port
        };
        let target = if local {
            &self.local_target
        } else {
            &self.target
        };
        egui::Frame::new()
            .fill(Color32::from_rgb(248, 249, 251))
            .corner_radius(6)
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.label(RichText::new("连接路径").size(12.).color(MUTED));
                ui.label(format!(
                    "{}:{}  →  {}  →  {}",
                    if ip.is_empty() { "待填写 IP" } else { ip },
                    if port.is_empty() {
                        "待填写端口"
                    } else {
                        port
                    },
                    if local {
                        "本地 Mac"
                    } else {
                        "所选虚拟机"
                    },
                    if page == Page::Socks {
                        "客户端指定目标"
                    } else if target.is_empty() {
                        "待填写实际目标"
                    } else {
                        target
                    }
                ));
            });
        ui.add_space(12.);
        egui::CollapsingHeader::new("连接说明与网络权限").show(ui,|ui|{
            ui.small("端口和实际目标由你填写，不自动推测服务。停止保留配置；删除会停止服务并移除规则。");
            if local {ui.small("Mac → 目标使用当前路由，仍可能受 VPN/TUN 影响。macOS 首次询问本地网络权限时，请允许 VNet Studio。");
                if ui.button("打开 Mac 本地网络权限").clicked(){let _=Command::new("/usr/bin/open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork").spawn();}
            }else{ui.small("监听地址属于所选虚拟机；在这里填写 127.0.0.1 只允许虚拟机内部访问。Mac 本地入口请在运行位置选择「本地 Mac」。");}
        });
    }
    fn rule_submit_ui(&mut self, ui: &mut egui::Ui, page: Page) {
        let local = page == Page::Forward && self.local_forward;
        let ready = local
            || self
                .vms
                .iter()
                .find(|g| g.name == self.vm)
                .map(|g| g.check().is_ok())
                .unwrap_or(false);
        let batch = if local {
            self.local_batch_mode
        } else {
            self.services.batch_mode
        };
        let complete = if batch {
            !(if local {
                &self.local_batch_text
            } else {
                &self.services.batch_text
            })
            .trim()
            .is_empty()
        } else if local {
            !self.local_port.trim().is_empty() && !self.local_target.trim().is_empty()
        } else if page == Page::Socks {
            !self.socks_port.trim().is_empty() && !self.listen_ip.trim().is_empty()
        } else {
            !self.port.trim().is_empty()
                && !self.target.trim().is_empty()
                && !self.listen_ip.trim().is_empty()
        };
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if !ready {
                    "请选择运行中的虚拟机"
                } else if !complete {
                    "填写完整配置后即可开启"
                } else {
                    "停止后保留规则，可随时重新开启"
                })
                .size(12.)
                .color(MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        self.rx.is_none() && ready && complete,
                        primary_button(if batch {
                            "＋  批量添加并开启"
                        } else {
                            "＋  添加并开启"
                        })
                        .min_size(egui::vec2(140., 34.)),
                    )
                    .clicked()
                {
                    self.submit_service_rule(page);
                }
            });
        });
    }
    fn workspace_result(&mut self, ui: &mut egui::Ui) {
        for (name, view) in [("Mac", &self.local_services), ("虚拟机", &self.services)] {
            if !view.error.is_empty() {
                ui.colored_label(
                    Color32::DARK_RED,
                    format!("{name} 状态读取失败：{}", view.error),
                );
            }
            if !view.result.is_empty() {
                egui::CollapsingHeader::new(format!("{name} · 操作结果"))
                    .id_salt(name)
                    .show(ui, |ui| {
                        ui.monospace(&view.result);
                    });
            }
        }
    }
    fn workspace_rule_actions(&mut self, ui: &mut egui::Ui, row: &serde_json::Value) {
        ui.horizontal(|ui| {
            if ui.button("＋ 新建规则").clicked() {
                self.forward_workspace.selected.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        row["managed"] == true && self.rx.is_none(),
                        primary_button(if row["running"] == true {
                            "停止转发"
                        } else {
                            "开启转发"
                        }),
                    )
                    .clicked()
                {
                    self.service_change(
                        row,
                        if row["running"] == true {
                            "stop"
                        } else {
                            "start"
                        },
                    );
                }
                if ui
                    .add_enabled(
                        row["managed"] == true && self.rx.is_none(),
                        egui::Button::new("删除规则"),
                    )
                    .clicked()
                {
                    self.service_change(row, "delete");
                }
            });
        });
    }
    fn workspace_rule_detail(&mut self, ui: &mut egui::Ui, row: &serde_json::Value) {
        let local = row["location"] == "host";
        let endpoint = format!("{}:{}", hostnet::field(row, "ip"), row["port"]);
        ui.horizontal(|ui| {
            ui.strong("规则详情");
            ui.colored_label(
                if row["running"] == true { GREEN } else { MUTED },
                if row["running"] == true {
                    "● 运行中"
                } else {
                    "● 已停止"
                },
            );
        });
        ui.add_space(22.);
        ui.columns(2, |cols| {
            cols[0].colored_label(BLUE, RichText::new("访问入口").size(16.).strong());
            cols[0].add_space(12.);
            cols[0].label(RichText::new(&endpoint).size(17.));
            cols[0].label(if local {
                "运行于：本地 Mac"
            } else {
                "运行于：所选虚拟机"
            });
            cols[1].colored_label(GREEN, RichText::new("实际目标").size(16.).strong());
            cols[1].add_space(12.);
            cols[1].label(
                RichText::new(if hostnet::field(row, "kind") == "socks" {
                    "由客户端指定"
                } else {
                    hostnet::field(row, "target")
                })
                .size(17.),
            );
            cols[1].label(if local {
                "由 Mac 发起目标连接"
            } else {
                "由虚拟机发起目标连接"
            });
        });
        ui.add_space(20.);
        ui.horizontal(|ui| {
            if ui.button("复制入口").clicked() {
                ui.ctx()
                    .copy_text(if hostnet::field(row, "kind") == "socks" {
                        endpoint.clone()
                    } else {
                        format!("http://{endpoint}")
                    });
            }
            if hostnet::field(row, "kind") == "forward"
                && ui
                    .add_enabled(row["running"] == true, egui::Button::new("打开入口 ↗"))
                    .clicked()
            {
                let _ = Command::new("/usr/bin/open")
                    .arg(format!("http://{endpoint}"))
                    .spawn();
            }
            if local
                && ui
                    .add_enabled(self.rx.is_none(), egui::Button::new("检查链路"))
                    .clicked()
            {
                self.service_change(row, "check");
            }
            ui.checkbox(
                if local {
                    &mut self.local_services.auto
                } else {
                    &mut self.services.auto
                },
                "自动刷新",
            );
        });
        ui.add_space(18.);
        ui.separator();
        ui.add_space(8.);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.forward_workspace.detail_tab, 0, "连接日志");
            ui.selectable_value(&mut self.forward_workspace.detail_tab, 1, "规则信息");
        });
        ui.add_space(14.);
        if self.forward_workspace.detail_tab == 1 {
            ui.label(format!("允许来源：{}", hostnet::field(row, "allow")));
            ui.label(format!("活跃连接：{}", row["stats"]["active"]));
            ui.label(format!(
                "累计发送：{} B   累计接收：{} B",
                row["stats"]["tx"], row["stats"]["rx"]
            ));
            ui.small("连接统计来自最近快照；规则允许不代表目标服务可达。");
            return;
        }
        let events = hostnet::array(&row["events"]);
        if events.is_empty() {
            ui.add_space(65.);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("暂无连接记录").color(MUTED));
                ui.small("有客户端连接入口后，记录将在这里显示");
            });
        } else {
            if ui.small_button("复制连接日志").clicked() {
                ui.ctx()
                    .copy_text(serde_json::to_string_pretty(&row["events"]).unwrap_or_default());
            }
            for e in events.iter().rev() {
                ui.label(
                    RichText::new(format!(
                        "{}   {}   {} → {}",
                        hostnet::field(e, "time"),
                        hostnet::field(e, "event"),
                        hostnet::field(e, "peer"),
                        hostnet::field(e, "target")
                    ))
                    .size(12.),
                );
                if let Some(error) = e["error"].as_str().filter(|s| !s.is_empty()) {
                    ui.colored_label(Color32::DARK_RED, error);
                }
                ui.separator();
            }
        }
        ui.small("仅记录连接元数据，不保存报文、URL 或密码。");
    }
}
