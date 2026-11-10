#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod gui;
mod toolbox;
use netbridge::{
    adapters,
    config::{self, Config},
    dial, proxy, system_proxy,
};
use std::{
    collections::VecDeque,
    io,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};

fn run() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "--watchdog") {
        let pid = args
            .get(1)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| io::Error::other("Missing parent PID"))?;
        let path = args
            .get(2)
            .ok_or_else(|| io::Error::other("Missing backup path"))?;
        let created = args
            .get(3)
            .map(|value| value.parse::<u64>().map_err(io::Error::other))
            .transpose()?;
        return system_proxy::watchdog_for(pid, std::path::Path::new(path), created);
    }
    let directory = config::data_dir();
    let path = args
        .iter()
        .position(|s| s == "--config")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.join("config.json"));
    let adapters = adapters::enumerate()?;
    let mut config = Config::load(&path)?;
    config.auto_select(&adapters);
    match args.first().map(String::as_str){
        Some("--tools")=>return toolbox::run(config),
        Some("--adb-info")=>{
            let installed=netbridge::adb_manager::ensure_bundled()?;
            let effective=netbridge::adb_manager::resolve(&config.adb_path)?;
            let output=netbridge::devices::command(effective.to_str().ok_or_else(||io::Error::other("Invalid ADB path"))?,&["version"])?;
            println!("{}",serde_json::json!({"bundled":installed,"effectivePath":effective,"versionOutput":output}));
        },
        Some("--adb-check-update")=>println!("{}",serde_json::to_string_pretty(&netbridge::adb_manager::check_update()?)?),
        Some("--adb-update")=>println!("{}",serde_json::to_string_pretty(&netbridge::adb_manager::update(&|text|eprintln!("{text}"))?)?),
        Some("--speed-test")=>{
            let choice=args.get(1).map(String::as_str).unwrap_or("all");
            let selected:Vec<_>=adapters.into_iter().filter(|a|choice=="all" || a.id==choice).collect();
            if selected.is_empty(){return Err(io::Error::other("No matching active adapter"))}
            let cancel=std::sync::atomic::AtomicBool::new(false);
            let rows:Vec<_>=selected.iter().map(|a|netbridge::speed::measure_adapter(a,&cancel,&|message|eprintln!("{message}"))).collect();
            println!("{}",serde_json::to_string_pretty(&rows)?);
        },
        Some("--adapters")=>println!("{}",serde_json::to_string_pretty(&adapters)?),
        Some("--check")=>{
            let runtime=tokio::runtime::Runtime::new()?;
            let results=runtime.block_on(async{
                let mut rows=Vec::new();
                for (host,port) in [("git.etc.vn",443),("10.3.6.5",12443),("www.onorca.dev",443),("103.122.82.124",1883)]{
                    let route=config.select(host);
                    match dial::connect(&config,host,port).await{
                        Ok((stream,_))=>rows.push(serde_json::json!({"host":host,"port":port,"route":route,"tcpConnected":true,"localAddress":stream.local_addr().ok().map(|s|s.ip())})),
                        Err(e)=>rows.push(serde_json::json!({"host":host,"port":port,"route":route,"tcpConnected":false,"error":e.to_string()})),
                    }
                }
                rows
            });
            println!("{}",serde_json::to_string_pretty(&results)?);
        },
        Some("--serve")=>{
            let logs=Arc::new(Mutex::new(VecDeque::new()));
            let server=proxy::Server::start(Arc::new(RwLock::new(config)),logs)?;
            println!("{}",serde_json::json!({"listening":format!("127.0.0.1:{}",server.port),"systemProxyChanged":false}));
            tokio::runtime::Runtime::new()?.block_on(tokio::signal::ctrl_c())?;
        },
        Some("--proxy-roundtrip")=>{
            let original=system_proxy::read()?;
            let snapshot=directory.join("roundtrip-backup.json");
            let _server=proxy::Server::start(Arc::new(RwLock::new(config.clone())),Arc::new(Mutex::new(VecDeque::new())))?;
            system_proxy::enable(&snapshot,config.port)?;
            let enabled=system_proxy::read()?.server==format!("127.0.0.1:{}",config.port);
            system_proxy::restore(&snapshot)?;
            let restored=system_proxy::read()?==original;
            println!("{}",serde_json::json!({"enabledVerified":enabled,"originalRestored":restored}));
            if !enabled || !restored{return Err(io::Error::other("Proxy roundtrip failed"))}
        },
        Some("--restore-proxy")=>{
            let snapshot=args.iter().position(|arg|arg=="--snapshot").and_then(|index|args.get(index+1))
                .map(PathBuf::from).unwrap_or_else(||directory.join("proxy-backup.json"));
            println!("restored={}",system_proxy::recover(&snapshot)?);
        },
        Some("--help")=>println!("NetBridge: GUI by default; --tools | --adb-info | --adb-check-update | --adb-update | --speed-test <GUID|all> | --adapters | --check | --serve [--config path] | --restore-proxy"),
        None=>return gui::run(config,path),
        _=>return Err(io::Error::other("Unknown option; use --help")),
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("NetBridge: {error}");
        if std::env::args().len() == 1 {
            gui::show_error(&error.to_string());
        }
        std::process::exit(1)
    }
}
