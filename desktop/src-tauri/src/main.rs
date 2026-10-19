#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{path::PathBuf,process::Command};
#[path="../../../src/hostnet.rs"] mod hostnet;
fn prl()->String{for p in ["/usr/local/bin/prlctl","/Applications/Parallels Desktop.app/Contents/MacOS/prlctl"]{if std::path::Path::new(p).exists(){return p.into();}}"prlctl".into()}
fn cli()->PathBuf{std::env::current_exe().unwrap_or_default().with_file_name("vnetctl-cli")}
fn run(purpose:String,args:Vec<String>)->Result<serde_json::Value,String>{
    if args.len()>1024||args.iter().any(|a|a.len()>131072)||args.iter().any(|a|a=="--prlctl"){return Err("无效请求参数".into());}
    let text=match purpose.as_str(){
        "vms"=>hostnet::command(&prl(),&["list".into(),"-a".into(),"-i".into(),"-j".into()])?,
        "host-read"|"host-preview"|"host-apply"=>hostnet::handle(&prl(),&purpose,&args)?,
        "cli"=>{
            let words=args.iter().filter(|s|!s.starts_with("--")).map(String::as_str).collect::<Vec<_>>();
            if !words.iter().any(|s|["host-forward","addresses","status","containers","routes","services","forward","socks","vpn"].contains(s))||args.iter().any(|s|s=="serve"){return Err("不支持的管理命令".into());}
            let mut a=args.clone();a.extend(["--prlctl".into(),prl()]);hostnet::command_with_timeout(&cli().to_string_lossy(),&a,std::time::Duration::from_secs(if args.first().map(String::as_str)==Some("host-forward")&&args.get(1).map(String::as_str)==Some("batch"){400}else{30}))?
        },_=>return Err("不支持的操作".into()),
    };
    Ok(serde_json::json!({"text":text,"data":serde_json::from_str::<serde_json::Value>(&text).ok()}))
}
#[tauri::command]
async fn execute(purpose:String,args:Vec<String>)->Result<serde_json::Value,String>{tauri::async_runtime::spawn_blocking(move||run(purpose,args)).await.map_err(|e|e.to_string())?}
#[tauri::command]
fn open_entry(url:String)->Result<(),String>{
    if !(url.starts_with("http://")||url.starts_with("https://"))||url.contains(['\n','\r'])||url.len()>4096{return Err("入口必须是 HTTP / HTTPS 网址".into());}
    Command::new("/usr/bin/open").arg(url).spawn().map_err(|e|e.to_string())?;Ok(())
}
#[tauri::command]
fn open_network_settings()->Result<(),String>{Command::new("/usr/bin/open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork").spawn().map_err(|e|e.to_string())?;Ok(())}
fn main(){tauri::Builder::default().invoke_handler(tauri::generate_handler![execute,open_entry,open_network_settings]).run(tauri::generate_context!()).expect("无法启动 VNet Studio");}
