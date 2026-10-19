use serde_json::Value;

#[derive(Clone, Debug)]
pub struct Guest {
    pub name: String,
    pub state: String,
    pub os: String,
    pub tools: String,
    pub id: String,
    pub ips: Vec<String>,
}
impl Guest {
    pub fn parse(v: &Value) -> Self {
        let text = |key: &str, alt: &str| {
            v.get(key)
                .or_else(|| v.get(alt))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string()
        };
        Self {
            name: text("Name", "name"),
            id: text("ID", "uuid"),
            ips: v
                .pointer("/Network/ipAddresses")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(|r| r.get("ip").and_then(Value::as_str))
                        .filter(|ip| {
                            ip.parse::<std::net::Ipv4Addr>()
                                .map(|a| a.is_private())
                                .unwrap_or(false)
                        })
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            state: text("State", "status"),
            os: text("OS", "os"),
            tools: v
                .pointer("/GuestTools/state")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .into(),
        }
    }
    pub fn windows(&self) -> bool {
        self.os.starts_with("win-") || self.os == "windows"
    }
    pub fn mac(&self) -> bool {
        self.os.starts_with("mac") || self.os.starts_with("osx")
    }
    pub fn unix(&self) -> bool {
        self.mac()
            || [
                "ubuntu", "debian", "kali", "fedora", "centos", "rhel", "redhat", "suse",
                "opensuse", "linux", "mint", "arch", "alpine", "rocky", "alma",
            ]
            .iter()
            .any(|s| self.os.starts_with(s))
    }
    pub fn check(&self) -> Result<(), String> {
        if !self.windows() && !self.unix() {
            return Err(format!(
                "尚不能识别 {} 的操作系统 {}。支持 Windows、Linux、macOS 来宾。",
                self.name, self.os
            ));
        }
        if self.state != "running" {
            return Err(format!("虚拟机 {} 当前状态为 {}。请先在 Parallels 中启动或恢复它，等待系统启动完成后重试；工具不会自动唤醒虚拟机。",self.name,self.state));
        }
        Ok(())
    }
    pub fn summary(&self) -> String {
        format!(
            "虚拟机：{}\nID：{}\n操作系统：{}\n运行状态：{}\nParallels Tools：{}\nPD 报告的私网 IP：{}",
            self.name, self.id, self.os, self.state, self.tools, self.ips.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognize_linux_regardless_of_name() {
        let g = Guest::parse(
            &serde_json::json!({"Name":"Windows fake","OS":"ubuntu","State":"running"}),
        );
        assert!(g.check().is_ok());
        assert!(g.unix());
        assert!(!g.windows());
    }
    #[test]
    fn reject_suspended_windows() {
        let g = Guest::parse(&serde_json::json!({"Name":"VM","OS":"win-11","State":"suspended"}));
        assert!(g.check().unwrap_err().contains("恢复"));
    }
    #[test]
    fn allow_running_windows() {
        let g = Guest::parse(&serde_json::json!({"OS":"win-11","State":"running"}));
        assert!(g.check().is_ok());
    }
    #[test]
    fn unknown_fails_closed() {
        assert!(Guest::parse(&serde_json::json!({})).check().is_err());
    }
    #[test]
    fn supports_mac() {
        let g = Guest::parse(&serde_json::json!({"OS":"macos","State":"running"}));
        assert!(g.mac() && g.check().is_ok());
    }
}
