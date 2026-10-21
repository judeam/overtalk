fn main() {
    if std::env::args().nth(1).as_deref() == Some("--build-identity") {
        println!("{}", env!("OVERTALK_WORKER_SOURCE_ID"));
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--isolation-probe") {
        use std::{
            fs,
            net::{SocketAddr, TcpStream},
            time::Duration,
        };
        let args = std::env::args().collect::<Vec<_>>();
        let read_allowed = fs::read_to_string(&args[2]).is_ok();
        let write_input = fs::write(&args[2], "modified").is_ok();
        let read_denied = fs::read_to_string(&args[3]).is_ok();
        let write_output = fs::write(&args[4], "permitted output").is_ok();
        let address: SocketAddr = args[5].parse().unwrap();
        let host_network = TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok();
        println!(
            "{}",
            serde_json::json!({"read_allowed":read_allowed,"write_input":write_input,"read_denied":read_denied,"write_output":write_output,"host_network":host_network,"inherited_secret":std::env::var_os("OVERTALK_TEST_SECRET").is_some()})
        );
        return;
    }
    match overtalk_worker_sandbox::run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("Isolated worker could not start: {error}");
            std::process::exit(126);
        }
    }
}
