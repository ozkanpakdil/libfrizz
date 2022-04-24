use ansi_term::Colour;
use clap::{load_yaml, App};
use dprint_core::formatting::PrintOptions;
use libfrizz::{execute_request, ExecRequest, TransportLayerProtocol};
use reqwest::{Method, Url};
use std::fs;
use std::process::exit;
use std::{
    cmp, env,
    fs::File,
    io,
    io::{Error, Write},
    net::{SocketAddr, ToSocketAddrs},
    path::Path,
};

mod port_details;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let frizz_version = env!("CARGO_PKG_VERSION");
    let yaml = load_yaml!("cli.yaml");
    let cmd_args = App::from(yaml)
        .version(env!("CARGO_PKG_VERSION"))
        .get_matches();

    if !cmd_args.is_present("target") {
        println!("\nERROR:Please provide the parameters\n");
        App::from(yaml)
            .version(env!("CARGO_PKG_VERSION"))
            .print_help().ok();
        exit(0);
    }
    let target = cmd_args.value_of("target").unwrap();

    let insecure = cmd_args.is_present("insecure");
    let verbose = cmd_args.is_present("verbose");
    if verbose {
        simple_logger::SimpleLogger::new()
            .with_utc_timestamps()
            .init()
            .unwrap();
    } else {
        simple_logger::SimpleLogger::new()
            .with_utc_timestamps()
            .with_level(log::LevelFilter::Info)
            .init()
            .unwrap();
    }
    port_details::init().await;
    let user_agent = if cmd_args.is_present("user-agent") {
        String::from(cmd_args.value_of("user-agent").unwrap())
    } else {
        format!("frizz / {}", frizz_version)
    };

    let mut method = match cmd_args.value_of("request") {
        Some(x) => Method::from_bytes(x.as_bytes()).unwrap(),
        _ => Method::GET,
    };

    // NOTE: if data given but method is GET, should be converted to POST
    if method.eq(&Method::GET) && cmd_args.is_present("data") {
        method = Method::POST;
    }
    if cmd_args.is_present("upload-file") {
        method = Method::PUT;
        log::debug!("method SET PUT.")
    }

    let mut out_writer = match cmd_args.value_of("output") {
        Some(x) => {
            let path = Path::new(x);
            Box::new(File::create(&path).unwrap()) as Box<dyn Write>
        }
        None => Box::new(io::stdout()) as Box<dyn Write>,
    };

    if target.starts_with("http") {
        match Url::parse(target) {
            Err(e) => {
                println!(
                    "Exiting because wrong url({}) and the reason is \"{}\".",
                    target, e
                );
                exit(0);
            }
            _ => {
                log::debug!("url is okay.")
            }
        }
        let mut _post_data = cmd_args.value_of("data").unwrap_or("").to_string();
        if cmd_args.is_present("upload-file") {
            let upload_file = cmd_args.value_of("upload-file").unwrap_or("");
            if fs::metadata(upload_file).unwrap().is_file() {
                _post_data = format!("@{}", upload_file);
            } else {
                log::error!("File not found,{}", upload_file);
                exit(-1);
            }
        }
        let res = execute_request(ExecRequest {
            url: target.to_string(),
            user_agent,
            verbose,
            disable_cert_validation: insecure,
            disable_hostname_validation: insecure,
            post_data: _post_data,
            http_method: method,
            progress_bar: cmd_args.is_present("progress-bar"),
        })
        .await
        .unwrap();
        let body = res.body;

        if cmd_args.is_present("fail") && !res.status_code.contains("200") {
            out_writer = Box::new(io::sink()) as Box<dyn Write>;
        }

        out_writer
            .write(format!("{}", Colour::Green.paint(res.status_code)).as_bytes())
            .ok();
        if cmd_args.is_present("dump-header") {
            out_writer
                .write(format!("{}", Colour::Blue.paint(res.headers)).as_bytes())
                .ok();
        }

        if cmd_args.is_present("pretty") {
            let opts = PrintOptions {
                indent_width: 4,
                max_width: 10,
                use_tabs: false,
                new_line_text: "\n",
            };
            let items = dprint_core::formatting::parser_helpers::parse_string(body.as_str());
            let out_prep = Colour::White.paint(dprint_core::formatting::format(|| items, opts));
            out_writer.write(out_prep.as_bytes()).ok();
        } else {
            out_writer
                .write(format!("{}", Colour::White.paint(body)).as_bytes())
                .ok();
        }
    } else if cmd_args.is_present("scan") {
        let socket_addresses: Vec<SocketAddr> =
            format!("{}:0", target).to_socket_addrs()?.collect();
        if socket_addresses.is_empty() {
            println!("Socket_addresses list is empty");
            return Ok(());
        }

        let timeout = cmd_args
            .value_of("timeout")
            .unwrap()
            .parse::<u64>()
            .unwrap_or(1);
        let concurrency = cmd_args
            .value_of("concurrency")
            .unwrap()
            .parse::<usize>()
            .unwrap_or(1024);

        let mut port1: u16 = 0;
        let mut port2: u16 = 0;
        let mut proto_opt = TransportLayerProtocol::None;
        if cmd_args.is_present("udp") {
            proto_opt = TransportLayerProtocol::Udp;
        } else if cmd_args.is_present("tcp") {
            proto_opt = TransportLayerProtocol::Tcp;
        } else if cmd_args.is_present("sctp") {
            proto_opt = TransportLayerProtocol::Sctp;
        }

        if cmd_args.is_present("ports") {
            let port_range: Vec<&str> = cmd_args.values_of("ports").unwrap().collect();
            if !port_range.is_empty() {
                port1 = port_range[0]
                    .parse()
                    .expect("Unexpected port entry: Enter a valid port number");
                port2 = port_range[1]
                    .parse()
                    .expect("Unexpected port entry: Enter a valid port number");
            }
        }

        libfrizz::scan(
            socket_addresses[0].ip(),
            concurrency,
            timeout,
            cmp::min(port1, port2),
            cmp::max(port1, port2),
            proto_opt,
            out_writer,
        )
        .await;
        return Ok(());
    } else {
        // assume telnet or any socket protocol
        libfrizz::open_socket_target(target).await.ok();
    }

    Ok(())
}
